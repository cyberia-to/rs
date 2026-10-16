use rustc_codegen_ssa::{CodegenResults, CrateInfo};
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::ty::{self, TyCtxt};

use crate::arm64::Arm64Backend;
use crate::lir::{LIROp, Label, Reg};
use crate::mir2lir::{MirToLir, StaticData, StaticReloc};
use link::{
    emit_macho, emit_object, CallReloc2, DataReloc, EmitInput, FnSymbol, RelocatableInput,
    StaticCodeReloc, StaticData as LinkData,
};

/// Generate LIR ops for the __trident_tls_get(key_ptr: *mut u64, size: usize) -> *mut u8 helper.
///
/// Lazily initialises a pthread_key_t slot and returns the per-thread data pointer:
///   1. If *key_ptr == 0: call pthread_key_create(key_ptr, NULL) to create the key.
///   2. Call pthread_getspecific(key) → ptr.
///   3. If ptr == NULL: malloc(size), zero it, pthread_setspecific(key, ptr).
///   4. Return ptr.
///
/// Callee-saved registers (x19-x24 = Reg(15)-Reg(20)) hold values across calls.
/// External symbols (_pthread_key_create, _pthread_getspecific, _pthread_setspecific, _malloc)
/// are resolved via libSystem stubs by the macho-linker; the monolithic path leaves them
/// unpatched (TLS correctness requires Phase 2+).
fn make_tls_get_ops() -> Vec<LIROp> {
    let saved_kp = Reg(15); // x19 — saved key_ptr across calls
    let saved_sz = Reg(16); // x20 — saved size across calls
    let key = Reg(17); // x21 — current pthread_key_t value
    let new_ptr = Reg(18); // x22 — per-thread data pointer
    let cond = Reg(19); // x23 — branch condition (0 or 1)
    let zero = Reg(20); // x24 — constant 0

    vec![
        LIROp::FnStart("__trident_tls_get".to_string()),
        // Save args to callee-saved regs; x0/x1 are caller-saved.
        LIROp::Move(saved_kp, Reg(0)),
        LIROp::Move(saved_sz, Reg(1)),
        LIROp::LoadImm(zero, 0),
        // key = *key_ptr
        LIROp::Load {
            dst: key,
            base: saved_kp,
            offset: 0,
        },
        LIROp::Eq(cond, key, zero),
        LIROp::Branch {
            cond,
            if_true: Label::new("__tls_init"),
            if_false: Label::new("__tls_have_key"),
        },
        // pthread_key_create(key_ptr, NULL)
        LIROp::LabelDef(Label::new("__tls_init")),
        LIROp::Move(Reg(0), saved_kp),
        LIROp::LoadImm(Reg(1), 0),
        LIROp::Call("pthread_key_create".to_string()),
        LIROp::Load {
            dst: key,
            base: saved_kp,
            offset: 0,
        },
        LIROp::Jump(Label::new("__tls_have_key")),
        // ptr = pthread_getspecific(key)
        LIROp::LabelDef(Label::new("__tls_have_key")),
        LIROp::Move(Reg(0), key),
        LIROp::Call("pthread_getspecific".to_string()),
        LIROp::Move(new_ptr, Reg(0)),
        LIROp::Eq(cond, new_ptr, zero),
        LIROp::Branch {
            cond,
            if_true: Label::new("__tls_alloc"),
            if_false: Label::new("__tls_done"),
        },
        // malloc(size) + zero + pthread_setspecific(key, ptr)
        LIROp::LabelDef(Label::new("__tls_alloc")),
        LIROp::Move(Reg(0), saved_sz),
        LIROp::Call("malloc".to_string()),
        LIROp::Move(new_ptr, Reg(0)),
        LIROp::Move(Reg(0), new_ptr),
        LIROp::Move(Reg(1), zero),
        LIROp::Move(Reg(2), saved_sz),
        LIROp::Call("__trident_memset".to_string()),
        // Reload key in case memset clobbered x21 via a call (it won't — memset is a stub,
        // but we reload defensively so the key is fresh from the key slot).
        LIROp::Load {
            dst: key,
            base: saved_kp,
            offset: 0,
        },
        LIROp::Move(Reg(0), key),
        LIROp::Move(Reg(1), new_ptr),
        LIROp::Call("pthread_setspecific".to_string()),
        LIROp::Jump(Label::new("__tls_done")),
        LIROp::LabelDef(Label::new("__tls_done")),
        LIROp::Move(Reg(0), new_ptr),
        LIROp::Return,
        LIROp::FnEnd,
    ]
}

/// Allocator shim stubs. rustc normally synthesizes the `___rustc` crate that
/// defines `__rust_alloc`/`__rust_dealloc`/`__rust_realloc`/`__rust_alloc_zeroed`
/// (names mangled as internal symbols) and forwards to the real allocator. This
/// backend skips that codegen, so emit equivalent stubs here, forwarding to
/// libSystem's malloc/free/realloc/calloc — the same implementation std's
/// default `System` allocator uses.
fn make_alloc_shim_ops(tcx: rustc_middle::ty::TyCtxt<'_>) -> Vec<LIROp> {
    use rustc_ast::expand::allocator::{global_fn_name, SpecialAllocatorMethod, ALLOCATOR_METHODS};
    let mut ops = Vec::new();
    for method in ALLOCATOR_METHODS {
        let sym = rustc_symbol_mangling::mangle_internal_symbol(tcx, &global_fn_name(method.name));
        ops.push(LIROp::FnStart(sym));
        match method.special {
            // __rust_alloc(size, _align) -> malloc(size)
            Some(SpecialAllocatorMethod::Alloc) => {
                ops.push(LIROp::Call("malloc".to_string()));
            }
            // __rust_dealloc(ptr, _size, _align) -> free(ptr)
            Some(SpecialAllocatorMethod::Dealloc) => {
                ops.push(LIROp::Call("free".to_string()));
            }
            // __rust_realloc(ptr, _size, _align, new_size) -> realloc(ptr, new_size)
            Some(SpecialAllocatorMethod::Realloc) => {
                ops.push(LIROp::Move(Reg(1), Reg(3)));
                ops.push(LIROp::Call("realloc".to_string()));
            }
            // __rust_alloc_zeroed(size, _align) -> calloc(1, size)
            Some(SpecialAllocatorMethod::AllocZeroed) => {
                ops.push(LIROp::Move(Reg(1), Reg(0)));
                ops.push(LIROp::LoadImm(Reg(0), 1));
                ops.push(LIROp::Call("calloc".to_string()));
            }
            None => {}
        }
        ops.push(LIROp::Return);
        ops.push(LIROp::FnEnd);
    }
    ops
}

/// Everything produced by per-crate codegen, passed to join_codegen.
pub struct TridentOutput {
    pub binary: Vec<u8>, // MH_EXECUTE monolithic binary
    pub object: Vec<u8>, // MH_OBJECT relocatable file
    pub crate_info: CrateInfo,
}

pub fn codegen_crate(tcx: TyCtxt<'_>) -> TridentOutput {
    let target_cpu = match tcx.sess.opts.cg.target_cpu {
        Some(ref n) => n.clone(),
        None => tcx.sess.target.cpu.as_ref().to_owned(),
    };

    let mono_items = tcx.collect_and_partition_mono_items(());

    let mut fn_items: Vec<_> = Vec::new();
    let mut static_defs: Vec<_> = Vec::new();

    for cgu in mono_items.codegen_units.iter() {
        for (item, _) in cgu.items() {
            match item {
                MonoItem::Fn(instance) => fn_items.push(*instance),
                MonoItem::Static(def_id) => static_defs.push(*def_id),
                MonoItem::GlobalAsm(_) => {}
            }
        }
    }

    // Entry function first.
    fn_items.sort_by(|a, b| {
        let a_name = tcx.symbol_name(*a).name.to_string();
        let b_name = tcx.symbol_name(*b).name.to_string();
        let a_entry = a_name == "main" || a_name == "_main";
        let b_entry = b_name == "main" || b_name == "_main";
        b_entry.cmp(&a_entry)
    });

    let mut all_ops: Vec<LIROp> = Vec::new();
    let mut all_statics: Vec<StaticData> = Vec::new();
    let mut all_static_relocs: Vec<StaticReloc> = Vec::new();

    // Lower static initializers to raw bytes, collecting function-pointer relocations.
    for def_id in &static_defs {
        let instance = ty::Instance::mono(tcx, *def_id);
        let name = tcx.symbol_name(instance).name.to_string();
        if let Ok(alloc) = tcx.eval_static_initializer(*def_id) {
            let inner = alloc.inner();
            let len = inner.len();
            let mut bytes = inner
                .inspect_with_uninit_and_ptr_outside_interpreter(0..len)
                .to_vec();
            // Collect function-pointer relocations (vtable entries).
            // Each provenance entry (offset, prov) records a pointer-sized slot.
            for (offset, prov) in inner.provenance().ptrs().iter() {
                let byte_off = offset.bytes() as usize;
                if byte_off + 8 > bytes.len() {
                    continue;
                }
                let prov_alloc_id = prov.alloc_id();
                use rustc_middle::mir::interpret::GlobalAlloc;
                match tcx.global_alloc(prov_alloc_id) {
                    GlobalAlloc::Function { instance: fn_inst } => {
                        let sym = tcx.symbol_name(fn_inst).name.to_string();
                        bytes[byte_off..byte_off + 8].fill(0);
                        all_static_relocs.push(StaticReloc {
                            static_name: name.clone(),
                            byte_offset: byte_off,
                            fn_symbol: sym,
                        });
                    }
                    GlobalAlloc::Static(target_def_id) => {
                        let target_inst = ty::Instance::mono(tcx, target_def_id);
                        let sym = tcx.symbol_name(target_inst).name.to_string();
                        bytes[byte_off..byte_off + 8].fill(0);
                        all_static_relocs.push(StaticReloc {
                            static_name: name.clone(),
                            byte_offset: byte_off,
                            fn_symbol: sym,
                        });
                    }
                    _ => {}
                }
            }
            let writable = tcx.is_mutable_static(*def_id);
            all_statics.push(StaticData {
                name,
                bytes,
                writable,
            });
        }
    }

    let mut all_tls_vars: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    // Fixpoint worklist: lowering instances can discover cross-crate-inlinable
    // callees that only this crate may define (instantiating-crate symbol suffix).
    let mut seen: std::collections::HashSet<String> = fn_items
        .iter()
        .map(|i| tcx.symbol_name(*i).name.to_string())
        .collect();
    let mut queue: std::collections::VecDeque<ty::Instance<'_>> =
        fn_items.iter().copied().collect();
    while let Some(instance) = queue.pop_front() {
        // Intrinsics with Rust implementations have no shim; read their MIR directly.
        let body = match instance.def {
            ty::InstanceKind::Intrinsic(def_id) => tcx.optimized_mir(def_id),
            _ => tcx.instance_mir(instance.def),
        };
        let mut lowerer = MirToLir::new(tcx, instance);
        match lowerer.lower(body, instance) {
            Ok(ops) => {
                if std::env::var("TRIDENT_DEBUG_LIR").is_ok() {
                    let name = tcx.symbol_name(instance).name.to_string();
                    let short = name.trim_start_matches('_');
                    for op in &ops {
                        eprintln!("LIR[{short}]: {op:?}");
                    }
                }
                all_ops.extend(ops);
                all_statics.extend(lowerer.take_statics());
                all_static_relocs.extend(lowerer.take_static_relocs());
                all_tls_vars.extend(lowerer.take_tls_vars());
                for extra in lowerer.take_extra_instances() {
                    let name = tcx.symbol_name(extra).name.to_string();
                    if seen.insert(name) {
                        queue.push_back(extra);
                    }
                }
            }
            Err(e) => {
                let fn_name = tcx.symbol_name(instance).name.to_string();
                tcx.sess
                    .dcx()
                    .warn(format!("mir2lir failed for {fn_name}: {e}"));
            }
        }
    }

    // C main entry wrapper. rustc relies on the codegen backend to emit it
    // (rustc_codegen_ssa::base::maybe_create_entry_fn): `main(argc, argv)`
    // calls the `start` lang item with the Rust main fn, argc, argv, and the
    // sigpipe attribute. Without it the linker entry falls back to the first
    // text function (lang_start), which misreads argc/argv as fn ptr/sigpipe.
    if let Some((main_def_id, entry_type)) = tcx.entry_fn(()) {
        use rustc_session::config::EntryFnType;
        if let EntryFnType::Main { sigpipe } = entry_type {
            let main_is_local = main_def_id.is_local();
            let main_instance = ty::Instance::mono(tcx, main_def_id);
            let in_this_unit = fn_items.contains(&main_instance);
            if (main_is_local && in_this_unit) || !main_is_local {
                let rust_main_sym = tcx.symbol_name(main_instance).name.to_string();
                // A #[no_mangle] extern "C" fn main IS the C entry already.
                if rust_main_sym != "main" && rust_main_sym != "_main" {
                    let env = ty::TypingEnv::fully_monomorphized();
                    let main_ret_ty = tcx
                        .fn_sig(main_def_id)
                        .instantiate_identity()
                        .skip_binder()
                        .output();
                    let main_ret_ty = tcx.normalize_erasing_regions(env, main_ret_ty);
                    let start_def_id =
                        tcx.require_lang_item(rustc_hir::LangItem::Start, rustc_span::DUMMY_SP);
                    let start_instance = ty::Instance::expect_resolve(
                        tcx,
                        env,
                        start_def_id,
                        tcx.mk_args(&[main_ret_ty.into()]),
                        rustc_span::DUMMY_SP,
                    );
                    let start_sym = tcx.symbol_name(start_instance).name.to_string();
                    let start_name = start_sym.clone();
                    if seen.insert(start_sym) {
                        queue.push_back(start_instance);
                    }
                    // main(argc: c_int /*x0*/, argv: *const *const c_char /*x1*/) -> c_int
                    let saved_argc = Reg(15); // x19 — callee-saved scratch
                    let saved_argv = Reg(16); // x20
                    all_ops.push(LIROp::FnStart("main".to_string()));
                    all_ops.push(LIROp::Move(saved_argc, Reg(0)));
                    all_ops.push(LIROp::Move(saved_argv, Reg(1)));
                    all_ops.push(LIROp::LoadAddr {
                        dst: Reg(0),
                        symbol: rust_main_sym,
                    });
                    all_ops.push(LIROp::Move(Reg(1), saved_argc));
                    all_ops.push(LIROp::Move(Reg(2), saved_argv));
                    all_ops.push(LIROp::LoadImm(Reg(3), sigpipe as u64));
                    all_ops.push(LIROp::Call(start_name));
                    all_ops.push(LIROp::Return);
                    all_ops.push(LIROp::FnEnd);
                }
            }
        }
    }

    // Emit 8-byte BSS key slot for each TLS variable.
    // The per-thread data is lazily malloc'd by __trident_tls_get; this slot holds the
    // pthread_key_t (0 = not yet created).
    if !all_tls_vars.is_empty() {
        for key_slot_sym in all_tls_vars.keys() {
            all_statics.push(StaticData {
                name: key_slot_sym.clone(),
                bytes: vec![0u8; 8],
                writable: true,
            });
        }
        all_ops.extend(make_tls_get_ops());
    }

    // Allocator shim: rustc synthesizes a `___rustc` crate defining `__rust_*` and
    // forwarding to the real allocator. Our backend never runs rustc's allocator
    // codegen, so emit the forwarding stubs ourselves.
    // Default kind (std's System allocator) forwards to malloc/free/realloc/calloc
    // in libSystem — matching System's semantics; a user `#[global_allocator]`
    // (kind Global) is not yet supported and would need `__rg_*` forwarding.
    if let Some(rustc_ast::expand::allocator::AllocatorKind::Default) = tcx.allocator_kind(()) {
        all_ops.extend(make_alloc_shim_ops(tcx));
        // The no-alloc-shim guard: alloc CRATES call this symbol as a function
        // (rustc's own shim emits a single `ret` in __text — a data symbol would
        // land in __cstring/__const and the call would execute string bytes).
        all_ops.push(LIROp::FnStart(
            rustc_symbol_mangling::mangle_internal_symbol(
                tcx,
                rustc_ast::expand::allocator::NO_ALLOC_SHIM_IS_UNSTABLE,
            ),
        ));
        all_ops.push(LIROp::Return);
        all_ops.push(LIROp::FnEnd);
        // `__rust_alloc_error_handler_should_panic`: a FUNCTION returning the
        // OOM-strategy flag (std calls it; no rlib defines it — the shim does).
        all_ops.push(LIROp::FnStart(
            rustc_symbol_mangling::mangle_internal_symbol(
                tcx,
                rustc_session::config::OomStrategy::SYMBOL,
            ),
        ));
        all_ops.push(LIROp::LoadImm(
            Reg(0),
            tcx.sess.opts.unstable_opts.oom.should_panic() as u64,
        ));
        all_ops.push(LIROp::Return);
        all_ops.push(LIROp::FnEnd);
    }

    let mut backend = Arm64Backend::new();
    let mut code = backend.lower(&all_ops);

    // Append syscall stubs for write and exit, then patch BL relocations.
    // macOS arm64: SYS_write=4, SYS_exit=1, trap via SVC #0x80 with x16=syscall#
    let write_stub_off = code.len();
    code.extend_from_slice(&0xD2800090u32.to_le_bytes()); // MOVZ x16, #4
    code.extend_from_slice(&0xD4001001u32.to_le_bytes()); // SVC  #0x80
    code.extend_from_slice(&0xD65F03C0u32.to_le_bytes()); // RET

    let exit_stub_off = code.len();
    code.extend_from_slice(&0xD2800030u32.to_le_bytes()); // MOVZ x16, #1
    code.extend_from_slice(&0xD4001001u32.to_le_bytes()); // SVC  #0x80
    code.extend_from_slice(&0xD4200000u32.to_le_bytes()); // BRK  #0

    // __trident_memcpy(x0=dst, x1=src, x2=count): byte-by-byte copy
    let memcpy_stub_off = code.len();
    code.extend_from_slice(&0xB40000A2u32.to_le_bytes()); // CBZ x2, done (+5 insns)
    code.extend_from_slice(&0x38401423u32.to_le_bytes()); // LDRB w3, [x1], #1
    code.extend_from_slice(&0x38001403u32.to_le_bytes()); // STRB w3, [x0], #1
    code.extend_from_slice(&0xF1000442u32.to_le_bytes()); // SUBS x2, x2, #1
    code.extend_from_slice(&0x54FFFFA1u32.to_le_bytes()); // B.NE loop (-3 insns)
    code.extend_from_slice(&0xD65F03C0u32.to_le_bytes()); // RET

    // __trident_memset(x0=dst, x1=val, x2=count): byte fill
    let memset_stub_off = code.len();
    code.extend_from_slice(&0xB4000082u32.to_le_bytes()); // CBZ x2, done (+4 insns)
    code.extend_from_slice(&0x38001401u32.to_le_bytes()); // STRB w1, [x0], #1
    code.extend_from_slice(&0xF1000442u32.to_le_bytes()); // SUBS x2, x2, #1
    code.extend_from_slice(&0x54FFFFC1u32.to_le_bytes()); // B.NE loop (-2 insns)
    code.extend_from_slice(&0xD65F03C0u32.to_le_bytes()); // RET

    // Snapshot unpatched code for the relocatable object (BL slots still 0x94000000).
    let unpatched_code = code.clone();

    for reloc in backend.call_relocs() {
        let target_off = match reloc.symbol.as_str() {
            "__trident_memcpy" | "_memcpy" | "memcpy" => memcpy_stub_off,
            "__trident_memset" | "_memset" | "memset" => memset_stub_off,
            "write" | "_write" => write_stub_off,
            "exit" | "_exit" => exit_stub_off,
            sym => {
                if let Some(&off) = backend.fn_offsets().get(sym) {
                    off
                } else {
                    tcx.sess
                        .dcx()
                        .warn(format!("unresolved call symbol: {sym}"));
                    continue;
                }
            }
        };
        let delta = (target_off as i64 - reloc.offset as i64) / 4;
        let patched = 0x94000000u32 | ((delta as u32) & 0x03FF_FFFF);
        code[reloc.offset..reloc.offset + 4].copy_from_slice(&patched.to_le_bytes());
    }

    let link_data: Vec<LinkData> = all_statics
        .into_iter()
        .map(|s| LinkData {
            name: s.name,
            bytes: s.bytes,
            writable: s.writable,
        })
        .collect();

    let link_relocs: Vec<DataReloc> = backend
        .data_relocs()
        .iter()
        .map(|r| DataReloc {
            adrp_offset: r.adrp_offset,
            add_offset: r.add_offset,
            symbol: r.symbol.clone(),
        })
        .collect();

    // Convert StaticReloc → StaticCodeReloc for the linker.
    let mut link_static_relocs: Vec<StaticCodeReloc> = all_static_relocs
        .iter()
        .map(|r| StaticCodeReloc {
            static_name: r.static_name.clone(),
            byte_offset: r.byte_offset,
            fn_symbol: r.fn_symbol.clone(),
        })
        .collect();

    // Darwin object symbols carry a leading underscore (LLVM convention):
    // `write` → `_write`, `_RNv...` → `__RNv...`. Prefix every symbol that
    // goes into the object/binary so our objects interoperate with LLVM-built
    // rlibs (sysroot std/core/alloc). Internal names (__trident_*, __carg_*,
    // vtables) are private to our objects; prefixing them is harmless.
    let macho_name = |name: &str| format!("_{name}");
    for r in &mut link_static_relocs {
        r.static_name = macho_name(&r.static_name);
        r.fn_symbol = macho_name(&r.fn_symbol);
    }

    // fn_offsets map for vtable patching in emit_macho.
    let mut fn_offsets_map: std::collections::HashMap<String, usize> = backend
        .fn_offsets()
        .iter()
        .map(|(k, &v)| (macho_name(k), v))
        .collect();
    fn_offsets_map.insert(macho_name("__trident_memcpy"), memcpy_stub_off);
    fn_offsets_map.insert(macho_name("__trident_memset"), memset_stub_off);

    // Build the relocatable object from the unpatched code (BL slots still 0x94000000).
    let mut fn_syms: Vec<FnSymbol> = backend
        .fn_offsets()
        .iter()
        .map(|(name, &offset)| FnSymbol {
            name: macho_name(name),
            offset,
            is_global: true,
        })
        .collect();
    fn_syms.push(FnSymbol {
        name: macho_name("__trident_memcpy"),
        offset: memcpy_stub_off,
        is_global: false,
    });
    fn_syms.push(FnSymbol {
        name: macho_name("__trident_memset"),
        offset: memset_stub_off,
        is_global: false,
    });

    let call_relocs2: Vec<CallReloc2> = backend
        .call_relocs()
        .iter()
        .map(|r| CallReloc2 {
            offset: r.offset,
            symbol: macho_name(&r.symbol),
        })
        .collect();

    let ro_data: Vec<LinkData> = link_data
        .iter()
        .filter(|d| !d.writable)
        .map(|d| LinkData {
            name: macho_name(&d.name),
            bytes: d.bytes.clone(),
            writable: false,
        })
        .collect();
    let rw_data: Vec<LinkData> = link_data
        .iter()
        .filter(|d| d.writable)
        .map(|d| LinkData {
            name: macho_name(&d.name),
            bytes: d.bytes.clone(),
            writable: true,
        })
        .collect();

    let rel_input = RelocatableInput {
        code: &unpatched_code,
        ro_data,
        rw_data,
        call_relocs: call_relocs2,
        data_relocs: link_relocs
            .iter()
            .map(|r| DataReloc {
                adrp_offset: r.adrp_offset,
                add_offset: r.add_offset,
                symbol: macho_name(&r.symbol),
            })
            .collect(),
        fn_syms,
        static_relocs: link_static_relocs.clone(),
    };
    let object = emit_object(&rel_input);

    let input = EmitInput {
        code: &code,
        data: link_data
            .iter()
            .map(|d| link::StaticData {
                name: macho_name(&d.name),
                bytes: d.bytes.clone(),
                writable: d.writable,
            })
            .collect(),
        relocs: link_relocs
            .iter()
            .map(|r| DataReloc {
                adrp_offset: r.adrp_offset,
                add_offset: r.add_offset,
                symbol: macho_name(&r.symbol),
            })
            .collect(),
        static_relocs: link_static_relocs,
        fn_offsets: fn_offsets_map,
        entry_offset: 0,
    };

    // The monolithic emitter can only bind symbols defined in this crate; when the
    // program references external functions through vtables (e.g. std's Debug impls),
    // it returns Err and the Mach-O linker path (dyld binding) must be used instead.
    let binary = emit_macho(&input).unwrap_or_default();

    TridentOutput {
        binary,
        object,
        crate_info: CrateInfo::new(tcx, target_cpu),
    }
}
