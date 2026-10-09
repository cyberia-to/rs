//! TLV descriptor finalization: rewrite __thread_vars thunks into the shape
//! dyld expects at load time.
//!
//! In a .o, each 24-byte thunk in `__DATA,__thread_vars` (S_THREAD_LOCAL_VARIABLES)
//! is `{ func: __tlv_bootstrap, key: 0, offset: <pointer to $tlv$init var> }`.
//! The static linker must transform it into `{ 0, 0, <offset within the coalesced
//! TLV template> }`, where the template is the run of contiguous
//! S_THREAD_LOCAL_REGULAR / S_THREAD_LOCAL_ZEROFILL sections (__thread_data +
//! __thread_bss). dyld (ThreadLocalVariables::initializeThunksFromDisk) validates
//! the offset against the template size, then rewrites the thunk itself:
//! func = &_tlv_get_addr, key = fresh pthread key, plus v2 fast-path fields.
//!
//! For dyld to run that setup at all, the mach header must carry
//! MH_HAS_TLV_DESCRIPTORS (set in emit when this pass reports thunks).

use crate::input::ObjData;
use crate::layout::Layout;

const S_THREAD_LOCAL_REGULAR: u32 = 0x11;
const S_THREAD_LOCAL_ZEROFILL: u32 = 0x12;
const S_THREAD_LOCAL_VARIABLES: u32 = 0x13;
/// Size of one tlv_descriptor/thunk: {func, key, offset}.
const THUNK_SIZE: usize = 24;

/// Rewrite all TLV thunks in the merged __DATA,__thread_vars section.
///
/// Must run after relocation application (the thunks' `offset` field then holds
/// the resolved absolute address of each `$tlv$init` variable) and before emit.
/// Returns true if any thunk was present (→ MH_HAS_TLV_DESCRIPTORS).
pub fn finalize_tlv(layout: &mut Layout, objects: &[ObjData]) -> Result<bool, String> {
    let Some(&vars_idx) = layout.sec_map.get("__DATA:__thread_vars") else {
        return Ok(false);
    };

    // Locate the TLV template sections (S_THREAD_LOCAL_REGULAR/ZEROFILL) and
    // verify they form one contiguous run — dyld accumulates the initial
    // content span over contiguous sections in load-command order.
    let mut template: Vec<(u64, u64)> = layout
        .merged
        .iter()
        .filter(|ms| {
            matches!(
                ms.flags & 0xFF,
                S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL
            )
        })
        .map(|ms| (ms.vm_addr, ms.virtual_size as u64))
        .collect();
    template.sort();
    for w in template.windows(2) {
        if w[0].0 + w[0].1 != w[1].0 {
            return Err(format!(
                "TLV template sections not contiguous: {:#x}+{:#x} then {:#x}",
                w[0].0, w[0].1, w[1].0
            ));
        }
    }
    let Some(&(template_base, _)) = template.first() else {
        return Err("TLV descriptors present but no TLV template sections".to_string());
    };
    let template_end = template.last().map(|&(vm, size)| vm + size).unwrap_or(template_base);
    let template_size = template_end - template_base;

    // Process each chunk's thunks; chunk boundaries matter because alignment
    // padding between merged chunks is not part of any thunk.
    let mut found = false;
    let chunks = layout.merged[vars_idx].chunks.clone();
    let data = &mut layout.merged[vars_idx].data;
    for chunk in &chunks {
        // Chunk length comes from the source section: padding between chunks
        // in the merged section is not part of any thunk.
        let chunk_len = objects[chunk.obj_idx].sections[chunk.sec_idx].data.len();
        let n_thunks = chunk_len / THUNK_SIZE;
        for t in 0..n_thunks {
            let off = chunk.offset_in_merged + t * THUNK_SIZE;
            let var_addr = u64::from_le_bytes(data[off + 16..off + 24].try_into().unwrap());
            if var_addr == 0 {
                // Padded/empty slot — leave zeros; dyld validates offsets, but a
                // zero offset with zero var is a valid "offset 0" only if the
                // template is non-empty; treat all-zero thunks as padding.
                if data[off..off + THUNK_SIZE].iter().all(|&b| b == 0) {
                    continue;
                }
                return Err(format!(
                    "TLV thunk at {}:{off:#x} has null offset field",
                    layout.merged[vars_idx].name
                ));
            }
            if var_addr < template_base || var_addr >= template_base + template_size {
                return Err(format!(
                    "TLV thunk at {}:{off:#x}: $tlv$init {var_addr:#x} outside template \
                     [{template_base:#x}, {:#x})",
                    layout.merged[vars_idx].name,
                    template_base + template_size
                ));
            }
            // {func: 0, key: 0, offset: template offset} — ld64's on-disk shape.
            data[off..off + 8].fill(0);
            data[off + 8..off + 16].fill(0);
            data[off + 16..off + 24].copy_from_slice(&(var_addr - template_base).to_le_bytes());
            found = true;
        }
    }

    Ok(found)
}
