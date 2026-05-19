//! Section layout: merge same-named sections from all objects, assign VAs.

use std::collections::HashMap;

use crate::input::ObjData;
use crate::resolve::{SymKind, SymbolTable};

/// VM base for the __TEXT segment (arm64 macOS default).
pub const VM_BASE: u64 = 0x1_0000_0000;
/// 16KB macOS arm64 page size.
pub const PAGE: usize = 16384;
/// 12 bytes per stub (ADRP x16 + LDR x16 + BR x16).
pub const STUB_SIZE: usize = 12;
/// 8 bytes per GOT entry.
pub const GOT_ENTRY_SIZE: usize = 8;

/// A merged output section with per-chunk tracking.
#[derive(Debug)]
pub struct MergedSection {
    pub seg: String,
    pub name: String,
    pub data: Vec<u8>,
    pub align: u32,       // log2 alignment
    pub flags: u32,
    pub vm_addr: u64,
    pub file_offset: usize,
    /// Contribution of each input object section.
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub obj_idx: usize,
    pub sec_idx: usize,
    /// Byte offset of this chunk within MergedSection.data.
    pub offset_in_merged: usize,
}

/// Output layout: merged sections + assigned addresses.
pub struct Layout {
    pub merged: Vec<MergedSection>,
    /// Index of __TEXT,__text in merged.
    pub text_idx: usize,
    /// Index of __TEXT,__stubs (synthesized), if any imports.
    pub stubs_idx: Option<usize>,
    /// Index of __DATA,__got (synthesized), if any imports.
    pub got_idx: Option<usize>,
    /// Ordered (seg, name) → merged index.
    pub sec_map: HashMap<String, usize>, // key: "seg:name"
    /// Per-object, per-section → merged index + chunk offset.
    pub obj_sec_map: HashMap<(usize, usize), (usize, usize)>,
    /// Addresses of each import stub.
    pub stub_addrs: Vec<u64>,
    /// Addresses of each GOT entry.
    pub got_addrs: Vec<u64>,
    /// File size of __TEXT segment (for code signing execSegLimit).
    pub text_filesize: usize,
    /// File offset of __DATA segment start (for dyld chained fixups).
    pub data_fileoff: usize,
    /// VM address of __DATA segment.
    pub data_vmaddr: u64,
    /// File offset of __LINKEDIT.
    pub linkedit_fileoff: usize,
    /// VM address of __LINKEDIT.
    pub linkedit_vmaddr: u64,
}

pub fn perform(
    objects: &[ObjData],
    syms: &mut SymbolTable,
    n_imports: usize,
) -> Layout {
    // ---- Step 1: merge sections from all objects ----
    let mut sec_map: HashMap<String, usize> = HashMap::new();
    let mut merged: Vec<MergedSection> = Vec::new();
    let mut obj_sec_map: HashMap<(usize, usize), (usize, usize)> = HashMap::new();

    for (obj_idx, obj) in objects.iter().enumerate() {
        for (sec_idx, sec) in obj.sections.iter().enumerate() {
            if sec.data.is_empty() && sec.relocs.is_empty() {
                // Empty section with no relocs — still need to map it in case symbols
                // reference it, but append nothing.
            }
            let key = format!("{}:{}", sec.seg, sec.name);
            let merged_idx = if let Some(&i) = sec_map.get(&key) {
                i
            } else {
                let i = merged.len();
                sec_map.insert(key.clone(), i);
                merged.push(MergedSection {
                    seg: sec.seg.clone(),
                    name: sec.name.clone(),
                    data: Vec::new(),
                    align: sec.align,
                    flags: sec.flags,
                    vm_addr: 0,
                    file_offset: 0,
                    chunks: Vec::new(),
                });
                i
            };
            let ms = &mut merged[merged_idx];
            // Ensure alignment.
            let align_bytes = 1usize << sec.align;
            let pad = align_bytes - (ms.data.len() % align_bytes);
            let pad = if pad == align_bytes { 0 } else { pad };
            let offset_in_merged = ms.data.len() + pad;
            ms.data.resize(offset_in_merged, 0);
            ms.data.extend_from_slice(&sec.data);
            ms.align = ms.align.max(sec.align);
            ms.chunks.push(Chunk { obj_idx, sec_idx, offset_in_merged });
            obj_sec_map.insert((obj_idx, sec_idx), (merged_idx, offset_in_merged));
        }
    }

    // ---- Step 2: add synthetic sections ----

    // __TEXT,__stubs (if needed)
    let stubs_idx = if n_imports > 0 {
        let stubs_data = vec![0u8; n_imports * STUB_SIZE];
        let key = "__TEXT:__stubs".to_string();
        let i = merged.len();
        sec_map.insert(key, i);
        merged.push(MergedSection {
            seg: "__TEXT".into(), name: "__stubs".into(),
            data: stubs_data, align: 2, // 4-byte aligned
            flags: 0x8000_0408, // S_SYMBOL_STUBS | PURE_INSTRUCTIONS | SOME_INSTRUCTIONS
            vm_addr: 0, file_offset: 0, chunks: Vec::new(),
        });
        Some(i)
    } else { None };

    // __DATA,__got (if needed)
    let got_idx = if n_imports > 0 {
        let got_data = vec![0u8; n_imports * GOT_ENTRY_SIZE];
        let key = "__DATA:__got".to_string();
        let i = merged.len();
        sec_map.insert(key, i);
        merged.push(MergedSection {
            seg: "__DATA".into(), name: "__got".into(),
            data: got_data, align: 3, // 8-byte aligned
            flags: 0x0000_0006, // S_NON_LAZY_SYMBOL_POINTERS
            vm_addr: 0, file_offset: 0, chunks: Vec::new(),
        });
        Some(i)
    } else { None };

    // ---- Step 3: sort sections into TEXT, DATA, LINKEDIT order ----
    // We need __TEXT,__text first (entry point), then other __TEXT, then __DATA.
    // Re-sort merged sections by segment priority.
    let seg_priority = |seg: &str| match seg {
        "__TEXT" => 0,
        "__DATA_CONST" => 1,
        "__DATA" => 2,
        _ => 3,
    };
    let sec_priority = |seg: &str, name: &str| -> u8 {
        match (seg, name) {
            ("__TEXT", "__text") => 0,
            ("__TEXT", "__stubs") => 1,
            ("__TEXT", "__stub_helper") => 2,
            ("__TEXT", "__cstring") => 3,
            ("__TEXT", "__const") => 4,
            ("__DATA", "__got") => 0,
            ("__DATA", "__data") => 1,
            ("__DATA", "__bss") => 2,
            ("__DATA", "__common") => 3,
            _ => 10,
        }
    };

    // Build a sort key for each section.
    let mut order: Vec<usize> = (0..merged.len()).collect();
    order.sort_by_key(|&i| {
        let ms = &merged[i];
        (seg_priority(&ms.seg), sec_priority(&ms.seg, &ms.name))
    });

    // Re-index sec_map and merged after sorting.
    let mut sorted_merged: Vec<MergedSection> = Vec::with_capacity(merged.len());
    let mut new_idx = vec![0usize; merged.len()];
    for (new_i, &old_i) in order.iter().enumerate() {
        new_idx[old_i] = new_i;
        // We take ownership by swapping with a dummy.
        let dummy = MergedSection {
            seg: String::new(), name: String::new(), data: Vec::new(),
            align: 0, flags: 0, vm_addr: 0, file_offset: 0, chunks: Vec::new(),
        };
        sorted_merged.push(std::mem::replace(&mut merged[old_i], dummy));
    }
    let merged = sorted_merged;

    // Update sec_map keys.
    let mut sec_map2: HashMap<String, usize> = HashMap::new();
    for (i, ms) in merged.iter().enumerate() {
        sec_map2.insert(format!("{}:{}", ms.seg, ms.name), i);
    }

    // Update obj_sec_map.
    let mut obj_sec_map2: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
    for ((obj_idx, sec_idx), (old_m, chunk_off)) in obj_sec_map {
        obj_sec_map2.insert((obj_idx, sec_idx), (new_idx[old_m], chunk_off));
    }

    let stubs_idx = stubs_idx.map(|i| new_idx[i]);
    let got_idx = got_idx.map(|i| new_idx[i]);
    let text_idx = sec_map2.get("__TEXT:__text").copied().unwrap_or(0);

    // ---- Step 4: assign VM addresses and file offsets ----
    // Layout: header bytes (not here), then __TEXT at VM_BASE, then __DATA page-aligned, __LINKEDIT.
    // File: header | __TEXT | __DATA | __LINKEDIT (all in file order).

    let header_size = compute_header_size(
        merged.iter().filter(|m| m.seg == "__TEXT").count(),
        merged.iter().filter(|m| m.seg == "__DATA" || m.seg == "__DATA_CONST").count(),
    );

    let code_file_offset = align_up(header_size, 16);
    let mut file_cursor = code_file_offset;
    let mut vm_cursor = VM_BASE + code_file_offset as u64;

    let mut text_end_fileoff = file_cursor;
    let mut data_start_fileoff = 0usize;
    let mut data_vmaddr = 0u64;
    let mut linkedit_fileoff = 0usize;
    let mut linkedit_vmaddr = 0u64;

    let mut prev_seg = "";
    let merged_len = merged.len();
    let mut merged = merged; // take mutable

    for i in 0..merged_len {
        let seg = merged[i].seg.clone();

        // Page-align on segment transition.
        if !seg.is_empty() && seg != prev_seg && prev_seg != "" {
            let paged = align_up(file_cursor, PAGE);
            let paged_vm = align_up_u64(vm_cursor, PAGE as u64);
            if prev_seg == "__TEXT" {
                text_end_fileoff = file_cursor;
                file_cursor = paged;
                vm_cursor = paged_vm;
                if seg == "__DATA" || seg == "__DATA_CONST" {
                    data_start_fileoff = file_cursor;
                    data_vmaddr = vm_cursor;
                }
            } else if (prev_seg == "__DATA" || prev_seg == "__DATA_CONST")
                && seg != "__DATA" && seg != "__DATA_CONST"
            {
                file_cursor = paged;
                vm_cursor = paged_vm;
                linkedit_fileoff = file_cursor;
                linkedit_vmaddr = vm_cursor;
            } else {
                file_cursor = paged;
                vm_cursor = paged_vm;
            }
        }
        if seg != prev_seg {
            prev_seg = Box::leak(seg.clone().into_boxed_str());
        }

        let align_bytes = 1usize << merged[i].align;
        file_cursor = align_up(file_cursor, align_bytes);
        vm_cursor = align_up_u64(vm_cursor, align_bytes as u64);

        merged[i].file_offset = file_cursor;
        merged[i].vm_addr = vm_cursor;

        file_cursor += merged[i].data.len();
        vm_cursor += merged[i].data.len() as u64;
    }

    // Handle the case where there are no __DATA sections.
    if text_end_fileoff == code_file_offset {
        text_end_fileoff = file_cursor;
    }
    if linkedit_fileoff == 0 {
        let paged = align_up(file_cursor, PAGE);
        linkedit_fileoff = paged;
        linkedit_vmaddr = align_up_u64(vm_cursor, PAGE as u64);
    }

    // ---- Step 5: update symbol addresses ----
    for sym in syms.syms.values_mut() {
        if let SymKind::Defined { obj_idx, sec_idx, offset } = sym.kind {
            if let Some(&(merged_idx, chunk_off)) = obj_sec_map2.get(&(obj_idx, sec_idx)) {
                sym.addr = merged[merged_idx].vm_addr + chunk_off as u64 + offset;
            }
        }
    }

    // Assign stub addresses and update DylibImport entries.
    let mut stub_addrs: Vec<u64> = Vec::new();
    let mut got_addrs: Vec<u64> = Vec::new();

    if let (Some(s_idx), Some(g_idx)) = (stubs_idx, got_idx) {
        let stubs_base = merged[s_idx].vm_addr;
        let got_base   = merged[g_idx].vm_addr;
        for i in 0..n_imports {
            stub_addrs.push(stubs_base + (i * STUB_SIZE) as u64);
            got_addrs.push(got_base + (i * GOT_ENTRY_SIZE) as u64);
        }
        // Update stub addresses in symbol table.
        for sym in syms.syms.values_mut() {
            if let SymKind::DylibImport { stub_idx, .. } = sym.kind {
                sym.addr = stub_addrs[stub_idx];
            }
        }
    }

    // ---- Step 6: encode stubs into the stubs section data ----
    if let Some(s_idx) = stubs_idx {
        let stubs_base = merged[s_idx].vm_addr;
        for i in 0..n_imports {
            let stub_va = stubs_base + (i * STUB_SIZE) as u64;
            let got_va  = got_addrs[i];
            let stub_bytes = encode_stub(stub_va, got_va);
            let off = i * STUB_SIZE;
            merged[s_idx].data[off..off + STUB_SIZE].copy_from_slice(&stub_bytes);
        }
    }

    let text_filesize = text_end_fileoff - code_file_offset + (code_file_offset - 0);
    // Actually text filesize = data_start_fileoff (everything before DATA is TEXT).
    let text_filesize_real = if data_start_fileoff > 0 {
        data_start_fileoff
    } else {
        linkedit_fileoff
    };

    Layout {
        merged,
        text_idx,
        stubs_idx,
        got_idx,
        sec_map: sec_map2,
        obj_sec_map: obj_sec_map2,
        stub_addrs,
        got_addrs,
        text_filesize: text_filesize_real,
        data_fileoff: data_start_fileoff,
        data_vmaddr,
        linkedit_fileoff,
        linkedit_vmaddr,
    }
}

fn encode_stub(stub_va: u64, got_va: u64) -> [u8; 12] {
    use crate::arm64::{ldr64_unsigned, br, patch_adrp};
    let pc_page = (stub_va & !0xFFF) as i64;
    let got_page = (got_va & !0xFFF) as i64;
    let page_delta = ((got_page - pc_page) >> 12) as i32;
    let page_offset = (got_va & 0xFFF) as u32;

    let insn0 = patch_adrp(0x90000010, page_delta); // adrp x16, page_delta
    let insn1 = ldr64_unsigned(16, 16, page_offset); // ldr x16, [x16, #page_offset]
    let insn2 = br(16);

    let mut out = [0u8; 12];
    out[0..4].copy_from_slice(&insn0.to_le_bytes());
    out[4..8].copy_from_slice(&insn1.to_le_bytes());
    out[8..12].copy_from_slice(&insn2.to_le_bytes());
    out
}

/// Estimate of header size for load command planning.
/// We overestimate to guarantee enough space; the actual sections are laid out at
/// align_up(header_size, 16).
fn compute_header_size(n_text_secs: usize, n_data_secs: usize) -> usize {
    const HEADER: usize = 32;
    const SEG: usize = 72;
    const SEC: usize = 80;
    const LDS: usize = 16; // LinkeditDataCommand
    // __PAGEZERO + __TEXT + sections + __DATA + sections + __LINKEDIT + fixed LCs
    HEADER
        + SEG          // __PAGEZERO
        + SEG + n_text_secs * SEC  // __TEXT
        + SEG + n_data_secs * SEC  // __DATA (if any)
        + SEG          // __LINKEDIT
        + LDS          // LC_DYLD_CHAINED_FIXUPS
        + LDS          // LC_DYLD_EXPORTS_TRIE
        + 24           // LC_SYMTAB
        + 80           // LC_DYSYMTAB
        + 32           // LC_LOAD_DYLINKER
        + 24           // LC_BUILD_VERSION
        + 24           // LC_MAIN
        + 56           // LC_LOAD_DYLIB libSystem
        + LDS          // LC_CODE_SIGNATURE
        + 256          // headroom for dylib list growth
}

#[inline]
pub fn align_up(x: usize, align: usize) -> usize {
    (x + align - 1) & !(align - 1)
}

#[inline]
fn align_up_u64(x: u64, align: u64) -> u64 {
    (x + align - 1) & !(align - 1)
}
