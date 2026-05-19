//! ARM64 relocation application: patch section bytes using resolved symbol addresses.

use crate::arm64;
use crate::input::{ObjData, RelocKind, RelocTarget};
use crate::layout::Layout;
use crate::resolve::SymbolTable;

pub fn apply_all(
    objects: &[ObjData],
    layout: &mut Layout,
    syms: &SymbolTable,
) -> Result<(), String> {
    for obj_idx in 0..objects.len() {
        let obj = &objects[obj_idx];
        for sec_idx in 0..obj.sections.len() {
            let sec = &obj.sections[sec_idx];
            if sec.relocs.is_empty() { continue; }

            let merged_entry = layout.obj_sec_map.get(&(obj_idx, sec_idx)).copied();
            let (merged_idx, chunk_off) = match merged_entry {
                Some(x) => x,
                None => continue,
            };

            let sec_vm = layout.merged[merged_idx].vm_addr + chunk_off as u64;
            let relocs = sec.relocs.clone();

            let mut i = 0;
            while i < relocs.len() {
                let r = &relocs[i];

                // SUBTRACTOR is paired with the next UNSIGNED reloc.
                if r.kind == RelocKind::Subtractor {
                    if i + 1 < relocs.len() && relocs[i + 1].kind == RelocKind::Unsigned {
                        apply_subtractor_pair(
                            r, &relocs[i + 1], sec_vm, merged_idx, chunk_off,
                            objects, layout, syms
                        )?;
                        i += 2;
                        continue;
                    } else {
                        eprintln!("warning: SUBTRACTOR not followed by UNSIGNED, skipping");
                        i += 1;
                        continue;
                    }
                }

                apply_one(r, sec_vm, merged_idx, chunk_off, objects, layout, syms)?;
                i += 1;
            }
        }
    }
    Ok(())
}

fn apply_one(
    r: &crate::input::Reloc,
    sec_vm: u64,
    merged_idx: usize,
    chunk_off: usize,
    objects: &[ObjData],
    layout: &mut Layout,
    syms: &SymbolTable,
) -> Result<(), String> {
    let pc = sec_vm + r.offset;
    let byte_off = chunk_off + r.offset as usize;

    // Resolve GOT address before taking mutable borrow of layout.merged.
    let got_addr_opt = match r.kind {
        RelocKind::GotPage21 | RelocKind::GotPageOff12 | RelocKind::PtrToGot => {
            Some(got_addr_for_target(&r.target, syms, layout)?)
        }
        _ => None,
    };

    let target_addr = resolve_target(&r.target, r.addend, pc, objects, layout, syms)?;

    let data = &mut layout.merged[merged_idx].data;

    match r.kind {
        RelocKind::Branch26 => {
            if byte_off + 4 > data.len() {
                return Err(format!("Branch26 reloc at {byte_off:#x} out of range"));
            }
            let insn = u32::from_le_bytes(data[byte_off..byte_off + 4].try_into().unwrap());
            let delta_bytes = target_addr as i64 - pc as i64;
            if delta_bytes % 4 != 0 {
                return Err(format!("Branch26 target not 4-aligned: {delta_bytes}"));
            }
            let delta = (delta_bytes / 4) as i32;
            if delta < -(1 << 25) || delta >= (1 << 25) {
                return Err(format!("Branch26 out of range: delta={delta} pc={pc:#x} target={target_addr:#x}"));
            }
            let patched = arm64::patch_branch26(insn, delta);
            data[byte_off..byte_off + 4].copy_from_slice(&patched.to_le_bytes());
        }

        RelocKind::Page21 => {
            if byte_off + 4 > data.len() {
                return Err(format!("Page21 reloc at {byte_off:#x} out of range"));
            }
            let insn = u32::from_le_bytes(data[byte_off..byte_off + 4].try_into().unwrap());
            let pc_page = (pc & !0xFFF) as i64;
            let target_page = (target_addr & !0xFFF) as i64;
            let page_delta = ((target_page - pc_page) >> 12) as i32;
            if page_delta < -(1 << 20) || page_delta >= (1 << 20) {
                return Err(format!("Page21 ADRP out of range: {page_delta}"));
            }
            let patched = arm64::patch_adrp(insn, page_delta);
            data[byte_off..byte_off + 4].copy_from_slice(&patched.to_le_bytes());
        }

        RelocKind::PageOff12 => {
            if byte_off + 4 > data.len() {
                return Err(format!("PageOff12 reloc at {byte_off:#x} out of range"));
            }
            let insn = u32::from_le_bytes(data[byte_off..byte_off + 4].try_into().unwrap());
            let page_offset = (target_addr & 0xFFF) as u32;
            let patched = if arm64::is_add_imm(insn) {
                arm64::patch_add_pageoff(insn, page_offset)
            } else {
                arm64::patch_ldr_pageoff(insn, page_offset)
            };
            data[byte_off..byte_off + 4].copy_from_slice(&patched.to_le_bytes());
        }

        RelocKind::GotPage21 => {
            if byte_off + 4 > data.len() {
                return Err(format!("GotPage21 reloc at {byte_off:#x} out of range"));
            }
            let insn = u32::from_le_bytes(data[byte_off..byte_off + 4].try_into().unwrap());
            let got_addr = got_addr_opt.unwrap();
            let pc_page = (pc & !0xFFF) as i64;
            let got_page = (got_addr & !0xFFF) as i64;
            let page_delta = ((got_page - pc_page) >> 12) as i32;
            let patched = arm64::patch_adrp(insn, page_delta);
            data[byte_off..byte_off + 4].copy_from_slice(&patched.to_le_bytes());
        }

        RelocKind::GotPageOff12 => {
            if byte_off + 4 > data.len() {
                return Err(format!("GotPageOff12 reloc at {byte_off:#x} out of range"));
            }
            let insn = u32::from_le_bytes(data[byte_off..byte_off + 4].try_into().unwrap());
            let got_addr = got_addr_opt.unwrap();
            let page_offset = (got_addr & 0xFFF) as u32;
            let patched = arm64::patch_ldr_pageoff(insn, page_offset);
            data[byte_off..byte_off + 4].copy_from_slice(&patched.to_le_bytes());
        }

        RelocKind::Unsigned => {
            let value = target_addr as i64 + r.addend;
            if r.size_bits == 64 {
                if byte_off + 8 > data.len() {
                    return Err(format!("Unsigned64 reloc at {byte_off:#x} out of range"));
                }
                data[byte_off..byte_off + 8].copy_from_slice(&(value as u64).to_le_bytes());
            } else {
                if byte_off + 4 > data.len() {
                    return Err(format!("Unsigned32 reloc at {byte_off:#x} out of range"));
                }
                data[byte_off..byte_off + 4].copy_from_slice(&(value as u32).to_le_bytes());
            }
        }

        RelocKind::PtrToGot => {
            if byte_off + 4 > data.len() {
                return Err(format!("PtrToGot reloc at {byte_off:#x} out of range"));
            }
            let got_addr = got_addr_opt.unwrap();
            let delta = got_addr as i64 - pc as i64;
            data[byte_off..byte_off + 4].copy_from_slice(&(delta as i32).to_le_bytes());
        }

        RelocKind::Subtractor => {}
    }
    Ok(())
}

fn apply_subtractor_pair(
    sub_r: &crate::input::Reloc,
    uns_r: &crate::input::Reloc,
    sec_vm: u64,
    merged_idx: usize,
    chunk_off: usize,
    objects: &[ObjData],
    layout: &mut Layout,
    syms: &SymbolTable,
) -> Result<(), String> {
    // SUBTRACTOR: address_of(sub_target)
    // UNSIGNED:   address_of(uns_target) + addend - address_of(sub_target)
    let sub_pc = sec_vm + sub_r.offset;
    let sub_target = resolve_target(&sub_r.target, 0, sub_pc, objects, layout, syms)?;

    let uns_pc = sec_vm + uns_r.offset;
    let uns_target = resolve_target(&uns_r.target, uns_r.addend, uns_pc, objects, layout, syms)?;

    let value = uns_target as i64 - sub_target as i64;
    let byte_off = chunk_off + uns_r.offset as usize;
    let data = &mut layout.merged[merged_idx].data;

    if uns_r.size_bits == 64 {
        if byte_off + 8 <= data.len() {
            data[byte_off..byte_off + 8].copy_from_slice(&(value as u64).to_le_bytes());
        }
    } else if uns_r.size_bits == 32 {
        if byte_off + 4 <= data.len() {
            data[byte_off..byte_off + 4].copy_from_slice(&(value as i32).to_le_bytes());
        }
    }
    Ok(())
}

fn resolve_target(
    target: &RelocTarget,
    addend: i64,
    _pc: u64,
    objects: &[ObjData],
    layout: &Layout,
    syms: &SymbolTable,
) -> Result<u64, String> {
    match target {
        RelocTarget::Symbol(name) => {
            if let Some(gs) = syms.syms.get(name.as_str()) {
                Ok((gs.addr as i64 + addend) as u64)
            } else {
                Err(format!("undefined symbol: {name}"))
            }
        }
        RelocTarget::SectionRel { seg, sec } => {
            let key = format!("{seg}:{sec}");
            if let Some(&merged_idx) = layout.sec_map.get(&key) {
                let base = layout.merged[merged_idx].vm_addr;
                // addend is the in-section offset (implicit from the instruction bytes).
                Ok((base as i64 + addend) as u64)
            } else {
                Err(format!("section reloc to unknown section {seg},{sec}"))
            }
        }
    }
}

fn got_addr_for_target(
    target: &RelocTarget,
    syms: &SymbolTable,
    layout: &Layout,
) -> Result<u64, String> {
    use crate::resolve::SymKind;
    let name = match target {
        RelocTarget::Symbol(n) => n.as_str(),
        _ => return Err("GOT reloc targets must be symbols".to_string()),
    };
    if let Some(gs) = syms.syms.get(name) {
        if let SymKind::DylibImport { got_idx, .. } = gs.kind {
            return Ok(layout.got_addrs[got_idx]);
        }
    }
    // If the symbol is locally defined and referenced via GOT (unusual but possible),
    // fall back to its direct address — this is non-standard but safe.
    if let Some(gs) = syms.syms.get(name) {
        return Ok(gs.addr);
    }
    Err(format!("GOT reloc: unknown symbol '{name}'"))
}
