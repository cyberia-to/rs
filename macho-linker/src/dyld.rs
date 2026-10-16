//! dyld chained fixups blob: LC_DYLD_CHAINED_FIXUPS data for the LINKEDIT segment.
//!
//! Format: dyld_chained_fixups_header → dyld_chained_starts_in_image →
//!         per-segment dyld_chained_starts_in_segment → imports → symbol names.
//!
//! Each fixup-bearing page gets ONE chain of 8-byte slots (binds and rebases
//! mixed; every slot self-describes via bit 63). Slots are 8-byte aligned and
//! linked with `next` in 4-byte stride units (next=2 → +8 bytes).
//!
//! Reference: Apple dyld_chained_fixups.h

// ---------------------------------------------------------------------------
// Constants from Apple dyld_chained_fixups.h
// ---------------------------------------------------------------------------

/// dyld_chained_fixups_header.imports_format: one 32-bit struct per import.
const DYLD_CHAINED_IMPORT: u32 = 1;
/// dyld_chained_starts_in_segment.pointer_format for 64-bit binaries.
const DYLD_CHAINED_PTR_64: u16 = 2;
/// Page size for macOS arm64 (16KB).
const CHAIN_PAGE_SIZE: u64 = 0x4000;
/// DYLD_CHAINED_PTR_START_NONE — page has no fixups.
const CHAIN_START_NONE: u16 = 0xFFFF;

/// A fixup-chain slot: either a dylib bind (import ordinal) or a rebase of a
/// statically-linked absolute pointer (vtable entry, fn-ptr static, local GOT).
/// dyld applies slide to rebase targets at load; without these, internal
/// pointers are wrong under ASLR.
#[derive(Debug, Clone)]
pub enum FixupSlot {
    /// 64-bit slot holding a chained-bind entry for import `ordinal`.
    Bind { vm_addr: u64, ordinal: usize },
    /// 64-bit slot holding a chained-rebase entry for a local `target` vmaddr
    /// (preferred base, pre-slide).
    Rebase { vm_addr: u64, target: u64 },
}

pub struct ChainedFixups {
    pub blob: Vec<u8>,
}

/// Encode a dyld_chained_ptr_64 bind entry for a slot.
fn bind_entry(ordinal: usize, next: u16) -> u64 {
    // bit 63: bind=1; bits [62:51]: next (4-byte units); bits [23:0]: ordinal.
    (1u64 << 63) | ((next as u64) << 51) | (ordinal as u64 & 0x00FF_FFFF)
}

/// Encode a dyld_chained_ptr_64 rebase entry for a slot.
fn rebase_entry(target: u64, next: u16) -> u64 {
    // bit 63: bind=0; bits [62:51]: next; bits [47:0]: target (pre-slide vmaddr).
    ((next as u64) << 51) | (target & 0x0000_FFFF_FFFF_FFFF)
}

/// Build the LC_DYLD_CHAINED_FIXUPS blob from the collected fixup slots.
///
/// `slots` — every 8-byte fixup slot (binds + rebases) by vm address.
/// `text_vmaddr`, `text_vmsize` — __TEXT segment bounds.
/// `data_vmaddr`, `data_vmsize` — __DATA segment bounds.
/// `imports` — symbol names (e.g. "_write") in GOT/stub order.
pub fn build(
    slots: &[FixupSlot],
    text_vmaddr: u64,
    text_vmsize: u64,
    data_vmaddr: u64,
    data_vmsize: u64,
    data_fileoff: u64,
    imports: &[String],
) -> ChainedFixups {
    build_impl(
        slots,
        text_vmaddr,
        text_vmsize,
        data_vmaddr,
        data_vmsize,
        data_fileoff,
        imports,
    )
}

fn build_impl(
    slots: &[FixupSlot],
    text_vmaddr: u64,
    text_vmsize: u64,
    data_vmaddr: u64,
    data_vmsize: u64,
    data_fileoff: u64,
    imports: &[String],
) -> ChainedFixups {
    let n = imports.len();

    // --- Group slots by (segment, page), sorted by vm address within a page.
    let seg_of = |vm: u64| -> Option<usize> {
        if vm >= text_vmaddr && vm < text_vmaddr + text_vmsize {
            Some(1) // __TEXT
        } else if vm >= data_vmaddr && vm < data_vmaddr + data_vmsize {
            Some(2) // __DATA
        } else {
            None
        }
    };
    use std::collections::BTreeMap;
    // (seg, page_index) -> sorted slot list
    let mut pages: BTreeMap<(usize, u64), Vec<&FixupSlot>> = BTreeMap::new();
    for s in slots {
        let vm = match s {
            FixupSlot::Bind { vm_addr, .. } | FixupSlot::Rebase { vm_addr, .. } => *vm_addr,
        };
        if let Some(seg) = seg_of(vm) {
            let page = (vm - if seg == 1 { text_vmaddr } else { data_vmaddr }) / CHAIN_PAGE_SIZE;
            pages.entry((seg, page)).or_default().push(s);
        }
    }

    // --- Layout: header(32) + starts_in_image(20) + per-seg starts +
    //     imports(4n) + symbol name pool.
    let mut seg_starts_offsets = [0u32; 4]; // [PAGEZERO, TEXT, DATA, LINKEDIT]
    let mut cursor: usize = 32 + 20;

    // Per-segment starts structs, packed CONTIGUOUSLY in segment order,
    // skipping segments without fixup pages (page-in linking assumes the
    // structs back each other without gaps).
    let mut seg_blobs: Vec<Vec<u8>> = Vec::new();
    for seg in 1..=2usize {
        let page_count = if seg == 1 {
            (text_vmsize + CHAIN_PAGE_SIZE - 1) / CHAIN_PAGE_SIZE
        } else {
            (data_vmsize + CHAIN_PAGE_SIZE - 1) / CHAIN_PAGE_SIZE
        };
        let seg_pages: Vec<(u64, &Vec<&FixupSlot>)> = pages
            .iter()
            .filter(|((s, _), _)| *s == seg)
            .map(|((_, p), v)| (*p, v))
            .collect();
        if seg_pages.is_empty() {
            continue;
        }
        // seg_info_offset is RELATIVE to the start of dyld_chained_starts_in_image
        // (which itself sits at file offset 32 in the blob).
        seg_starts_offsets[seg] = (cursor - 32) as u32;
        // size = 22 + 2*page_count
        let size = 22 + 2 * page_count as usize;
        cursor += size;
        let mut blob = Vec::with_capacity(size);
        let w = |blob: &mut Vec<u8>, v: u32| blob.extend_from_slice(&v.to_le_bytes());
        let w16 = |blob: &mut Vec<u8>, v: u16| blob.extend_from_slice(&v.to_le_bytes());
        w(&mut blob, size as u32);
        w16(&mut blob, CHAIN_PAGE_SIZE as u16);
        w16(&mut blob, DYLD_CHAINED_PTR_64);
        // segment_offset is the segment's FILE offset (0 for __TEXT) — dyld
        // locates slots as segment_offset + page*page_size + page_start.
        blob.extend_from_slice(&if seg == 1 { 0u64 } else { data_fileoff }.to_le_bytes());
        w(&mut blob, 0); // max_valid_pointer
        w16(&mut blob, page_count as u16);
        for p in 0..page_count {
            // page_start = offset of the LOWEST slot on the page (slots are
            // sorted by vm within the page map value via insertion order of
            // the grouped vec — sort here to be exact).
            let off_in_page = seg_pages
                .iter()
                .find(|(pg, _)| *pg == p)
                .map(|(_, slots)| {
                    let base = if seg == 1 { text_vmaddr } else { data_vmaddr };
                    let min_vm = slots.iter().map(|s| s.vm_addr()).min().unwrap();
                    (min_vm - (base + p * CHAIN_PAGE_SIZE)) as u16
                })
                .unwrap_or(CHAIN_START_NONE);
            w16(&mut blob, off_in_page);
        }
        seg_blobs.push(blob);
    }

    // --- Imports + names.
    let imports_offset = cursor as u32;
    // On macOS, C-ABI symbols need a leading underscore in dyld import tables.
    let mut names: Vec<u8> = Vec::new();
    let mut name_offsets: Vec<u32> = Vec::new();
    for import in imports {
        name_offsets.push(names.len() as u32);
        if !import.starts_with('_') {
            names.push(b'_');
        }
        names.extend_from_slice(import.as_bytes());
        names.push(0);
    }
    while names.len() % 4 != 0 {
        names.push(0);
    }
    let symbols_offset = imports_offset + n as u32 * 4;

    // --- Assemble.
    let total = symbols_offset as usize + names.len();
    let mut blob = vec![0u8; total];
    let mut w = Writer::new(&mut blob);

    // dyld_chained_fixups_header (32 bytes)
    w.u32(0); // fixups_version
    w.u32(32); // starts_offset
    w.u32(imports_offset); // imports_offset
    w.u32(symbols_offset); // symbols_offset
    w.u32(n as u32); // imports_count
    w.u32(DYLD_CHAINED_IMPORT); // imports_format
    w.u32(0); // symbols_format (0 = uncompressed)
    w.u32(0); // reserved

    // dyld_chained_starts_in_image: seg_count=4 + 4 offsets
    w.u32(4);
    for off in seg_starts_offsets {
        w.u32(off);
    }

    for seg_blob in &seg_blobs {
        w.bytes(seg_blob);
    }

    // dyld_chained_import entries: lib_ordinal=1, weak=0, name_offset<<9
    for (i, _) in imports.iter().enumerate() {
        let entry: u32 = 1 | (name_offsets[i] << 9);
        w.u32(entry);
    }
    w.bytes(&names);

    debug_assert_eq!(w.pos, total);
    let _ = w;

    ChainedFixups { blob }
}

impl FixupSlot {
    pub fn vm_addr(&self) -> u64 {
        match self {
            FixupSlot::Bind { vm_addr, .. } | FixupSlot::Rebase { vm_addr, .. } => *vm_addr,
        }
    }
    /// Encode the on-disk 64-bit chain entry; `next` links to the following
    /// slot in the page chain (0 ends the chain).
    pub fn encode(&self, next: u16) -> u64 {
        match self {
            FixupSlot::Bind { ordinal, .. } => bind_entry(*ordinal, next),
            FixupSlot::Rebase { target, .. } => rebase_entry(*target, next),
        }
    }
}

// ---------------------------------------------------------------------------
// Minimal byte writer
// ---------------------------------------------------------------------------

struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn u32(&mut self, v: u32) {
        self.buf[self.pos..self.pos + 4].copy_from_slice(&v.to_le_bytes());
        self.pos += 4;
    }
    fn u16(&mut self, v: u16) {
        self.buf[self.pos..self.pos + 2].copy_from_slice(&v.to_le_bytes());
        self.pos += 2;
    }
    fn bytes(&mut self, v: &[u8]) {
        self.buf[self.pos..self.pos + v.len()].copy_from_slice(v);
        self.pos += v.len();
    }
}
