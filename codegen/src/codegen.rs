use rustc_codegen_ssa::{CodegenResults, CrateInfo};
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::ty::{self, TyCtxt};

use crate::arm64::Arm64Backend;
use crate::lir::LIROp;
use crate::mir2lir::{MirToLir, StaticData};
use link::{DataReloc, EmitInput, StaticData as LinkData, emit_macho};

/// Everything produced by per-crate codegen, passed to join_codegen.
pub struct TridentOutput {
    pub binary: Vec<u8>,
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

    // Lower static initializers to raw bytes.
    for def_id in &static_defs {
        let instance = ty::Instance::mono(tcx, *def_id);
        let name = tcx.symbol_name(instance).name.to_string();
        if let Ok(alloc) = tcx.eval_static_initializer(*def_id) {
            let inner = alloc.inner();
            let len = inner.len();
            let bytes = inner
                .inspect_with_uninit_and_ptr_outside_interpreter(0..len)
                .to_vec();
            all_statics.push(StaticData { name, bytes });
        }
    }

    for instance in &fn_items {
        let body = tcx.instance_mir(instance.def);
        let mut lowerer = MirToLir::new(tcx);
        match lowerer.lower(body, *instance) {
            Ok(ops) => {
                all_ops.extend(ops);
                all_statics.extend(lowerer.take_statics());
            }
            Err(e) => {
                let fn_name = tcx.symbol_name(*instance).name.to_string();
                tcx.sess.dcx().warn(format!("mir2lir failed for {fn_name}: {e}"));
            }
        }
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

    for reloc in backend.call_relocs() {
        let target_off = match reloc.symbol.as_str() {
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
        .map(|s| LinkData { name: s.name, bytes: s.bytes })
        .collect();

    let link_relocs: Vec<DataReloc> = backend.data_relocs()
        .iter()
        .map(|r| DataReloc {
            adrp_offset: r.adrp_offset,
            add_offset: r.add_offset,
            symbol: r.symbol.clone(),
        })
        .collect();

    let input = EmitInput {
        code: &code,
        data: link_data,
        relocs: link_relocs,
        entry_offset: 0,
    };

    let binary = emit_macho(&input);

    TridentOutput {
        binary,
        crate_info: CrateInfo::new(tcx, target_cpu),
    }
}
