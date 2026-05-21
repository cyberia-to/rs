use rustc_codegen_ssa::{CodegenResults, CrateInfo};
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::ty::{self, TyCtxt};

use crate::arm64::Arm64Backend;
use crate::lir::LIROp;
use crate::mir2lir::{MirToLir, StaticData, StaticReloc};
use link::{CallReloc2, DataReloc, EmitInput, FnSymbol, RelocatableInput, StaticCodeReloc, StaticData as LinkData, emit_macho, emit_object};

/// Everything produced by per-crate codegen, passed to join_codegen.
pub struct TridentOutput {
    pub binary:     Vec<u8>,   // MH_EXECUTE monolithic binary
    pub object:     Vec<u8>,   // MH_OBJECT relocatable file
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
                if byte_off + 8 > bytes.len() { continue; }
                let prov_alloc_id = prov.alloc_id();
                use rustc_middle::mir::interpret::GlobalAlloc;
                match tcx.global_alloc(prov_alloc_id) {
                    GlobalAlloc::Function { instance: fn_inst } => {
                        let sym = tcx.symbol_name(fn_inst).name.to_string();
                        bytes[byte_off..byte_off + 8].fill(0);
                        all_static_relocs.push(StaticReloc {
                            static_name: name.clone(),
                            byte_offset: byte_off,
                            fn_symbol:   sym,
                        });
                    }
                    GlobalAlloc::Static(target_def_id) => {
                        let target_inst = ty::Instance::mono(tcx, target_def_id);
                        let sym = tcx.symbol_name(target_inst).name.to_string();
                        bytes[byte_off..byte_off + 8].fill(0);
                        all_static_relocs.push(StaticReloc {
                            static_name: name.clone(),
                            byte_offset: byte_off,
                            fn_symbol:   sym,
                        });
                    }
                    _ => {}
                }
            }
            let writable = tcx.is_mutable_static(*def_id);
            all_statics.push(StaticData { name, bytes, writable });
        }
    }

    let mut all_tls_vars: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    for instance in &fn_items {
        let body = tcx.instance_mir(instance.def);
        let mut lowerer = MirToLir::new(tcx, *instance);
        match lowerer.lower(body, *instance) {
            Ok(ops) => {
                all_ops.extend(ops);
                all_statics.extend(lowerer.take_statics());
                all_static_relocs.extend(lowerer.take_static_relocs());
                all_tls_vars.extend(lowerer.take_tls_vars());
            }
            Err(e) => {
                let fn_name = tcx.symbol_name(*instance).name.to_string();
                tcx.sess.dcx().warn(format!("mir2lir failed for {fn_name}: {e}"));
            }
        }
    }

    // Emit writable BSS storage for each TLS variable.
    for (sym, size) in &all_tls_vars {
        all_statics.push(StaticData { name: sym.clone(), bytes: vec![0u8; *size as usize], writable: true });
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
            "exit"  | "_exit"  => exit_stub_off,
            sym => {
                if let Some(&off) = backend.fn_offsets().get(sym) {
                    off
                } else {
                    tcx.sess.dcx().warn(format!("unresolved call symbol: {sym}"));
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
        .map(|s| LinkData { name: s.name, bytes: s.bytes, writable: s.writable })
        .collect();

    let link_relocs: Vec<DataReloc> = backend.data_relocs()
        .iter()
        .map(|r| DataReloc {
            adrp_offset: r.adrp_offset,
            add_offset: r.add_offset,
            symbol: r.symbol.clone(),
        })
        .collect();

    // Convert StaticReloc → StaticCodeReloc for the linker.
    let link_static_relocs: Vec<StaticCodeReloc> = all_static_relocs
        .iter()
        .map(|r| StaticCodeReloc {
            static_name: r.static_name.clone(),
            byte_offset: r.byte_offset,
            fn_symbol:   r.fn_symbol.clone(),
        })
        .collect();

    // fn_offsets map for vtable patching in emit_macho.
    let mut fn_offsets_map: std::collections::HashMap<String, usize> = backend.fn_offsets()
        .iter()
        .map(|(k, &v)| (k.clone(), v))
        .collect();
    fn_offsets_map.insert("__trident_memcpy".to_string(), memcpy_stub_off);
    fn_offsets_map.insert("__trident_memset".to_string(), memset_stub_off);

    // Build the relocatable object from the unpatched code (BL slots still 0x94000000).
    let mut fn_syms: Vec<FnSymbol> = backend.fn_offsets()
        .iter()
        .map(|(name, &offset)| FnSymbol { name: name.clone(), offset, is_global: true })
        .collect();
    fn_syms.push(FnSymbol { name: "__trident_memcpy".to_string(), offset: memcpy_stub_off, is_global: false });
    fn_syms.push(FnSymbol { name: "__trident_memset".to_string(), offset: memset_stub_off, is_global: false });

    let call_relocs2: Vec<CallReloc2> = backend.call_relocs()
        .iter()
        .map(|r| CallReloc2 { offset: r.offset, symbol: r.symbol.clone() })
        .collect();

    let ro_data: Vec<LinkData> = link_data.iter().filter(|d| !d.writable)
        .map(|d| LinkData { name: d.name.clone(), bytes: d.bytes.clone(), writable: false })
        .collect();
    let rw_data: Vec<LinkData> = link_data.iter().filter(|d| d.writable)
        .map(|d| LinkData { name: d.name.clone(), bytes: d.bytes.clone(), writable: true })
        .collect();

    let rel_input = RelocatableInput {
        code:        &unpatched_code,
        ro_data,
        rw_data,
        call_relocs: call_relocs2,
        data_relocs: link_relocs.iter()
            .map(|r| DataReloc { adrp_offset: r.adrp_offset, add_offset: r.add_offset, symbol: r.symbol.clone() })
            .collect(),
        fn_syms,
    };
    let object = emit_object(&rel_input);

    let input = EmitInput {
        code: &code,
        data: link_data,
        relocs: link_relocs,
        static_relocs: link_static_relocs,
        fn_offsets: fn_offsets_map,
        entry_offset: 0,
    };

    let binary = emit_macho(&input);

    TridentOutput {
        binary,
        object,
        crate_info: CrateInfo::new(tcx, target_cpu),
    }
}
