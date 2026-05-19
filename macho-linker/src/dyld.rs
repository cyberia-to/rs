//! dyld chained fixups blob: LC_DYLD_CHAINED_FIXUPS data for the LINKEDIT segment.
//!
//! Format: dyld_chained_fixups_header → dyld_chained_starts_in_image →
//!         dyld_chained_starts_in_segment (DATA) → imports → symbol names.
//!
//! Reference: Apple xnu dyld_chained_fixups.h

// ---------------------------------------------------------------------------
// Constants from Apple dyld_chained_fixups.h
// ---------------------------------------------------------------------------

/// dyld_chained_fixups_header.imports_format: one 32-bit struct per import.
const DYLD_CHAINED_IMPORT: u32 = 1;
/// dyld_chained_starts_in_segment.pointer_format for 64-bit binaries.
const DYLD_CHAINED_PTR_64: u16 = 2;
/// Stride for DYLD_CHAINED_PTR_64 next field: 4 bytes.
const CHAIN_STRIDE: u64 = 4;
/// Page size for macOS arm64 (16KB).
const CHAIN_PAGE_SIZE: u16 = 0x4000;

pub struct ChainedFixups {
    pub blob: Vec<u8>,
    /// Byte offset of the GOT section's first entry within its DATA page.
    /// Used to set page_start[0] in dyld_chained_starts_in_segment.
    pub got_page_start: u16,
}

/// Build the LC_DYLD_CHAINED_FIXUPS blob.
///
/// `imports` — symbol names (e.g., "_write") in GOT/stub order.
/// `got_fileoff` — file offset of __DATA,__got section start.
/// `data_fileoff` — file offset of __DATA segment start.
pub fn build(
    imports: &[String],
    got_fileoff: usize,
    data_fileoff: usize,
    n_data_sections: usize, // number of sections in __DATA segment
) -> ChainedFixups {
    let n = imports.len();

    // --- Sizes ---
    // dyld_chained_starts_in_image: 4 (seg_count) + 4*4 (offsets for 4 segs).
    const STARTS_IMAGE_BASE: usize = 4 + 4 * 4; // 20 bytes

    // dyld_chained_starts_in_segment with 1 page: 22 + 2*1 = 24 bytes.
    const STARTS_SEG_SIZE: usize = 24;

    let imports_offset: u32 = (32 + STARTS_IMAGE_BASE + STARTS_SEG_SIZE) as u32;
    let symbols_offset: u32 = imports_offset + n as u32 * 4;

    // Build symbol-names area.
    let mut names: Vec<u8> = Vec::new();
    let mut name_offsets: Vec<u32> = Vec::new();
    for import in imports {
        name_offsets.push(names.len() as u32);
        names.extend_from_slice(import.as_bytes());
        names.push(0);
    }
    // Pad names to 4-byte alignment.
    while names.len() % 4 != 0 { names.push(0); }

    let total = 32 + STARTS_IMAGE_BASE + STARTS_SEG_SIZE + n * 4 + names.len();
    let mut blob = vec![0u8; total];
    let mut w = Writer::new(&mut blob);

    // --- dyld_chained_fixups_header (32 bytes) ---
    w.u32(0);                  // fixups_version
    w.u32(32);                 // starts_offset (right after 32-byte header)
    w.u32(imports_offset);     // imports_offset
    w.u32(symbols_offset);     // symbols_offset
    w.u32(n as u32);           // imports_count
    w.u32(DYLD_CHAINED_IMPORT);// imports_format
    w.u32(0);                  // symbols_format (0 = uncompressed)
    w.u32(0);                  // reserved

    debug_assert_eq!(w.pos, 32);

    // --- dyld_chained_starts_in_image (STARTS_IMAGE_BASE bytes = 20) ---
    // seg_count = 4: PAGEZERO, TEXT, DATA, LINKEDIT
    w.u32(4);
    w.u32(0); // PAGEZERO — no fixups
    w.u32(0); // TEXT — no fixups (RO)
    w.u32(STARTS_IMAGE_BASE as u32); // DATA — starts_in_segment right after this struct
    w.u32(0); // LINKEDIT — no fixups

    debug_assert_eq!(w.pos, 32 + STARTS_IMAGE_BASE);

    // --- dyld_chained_starts_in_segment for DATA (24 bytes) ---
    let got_page_start = (got_fileoff - data_fileoff) as u16; // offset within DATA page
    w.u32(STARTS_SEG_SIZE as u32); // size
    w.u16(CHAIN_PAGE_SIZE);        // page_size
    w.u16(DYLD_CHAINED_PTR_64);   // pointer_format
    w.u64(data_fileoff as u64);    // segment_offset (from start of file)
    w.u32(0);                      // max_valid_pointer (0 = no limit)
    w.u16(if n > 0 { 1 } else { 0 }); // page_count
    // page_start[0]: byte offset of first GOT entry within the first DATA page.
    if n > 0 {
        w.u16(got_page_start);
    } else {
        w.u16(0xFFFF); // DYLD_CHAINED_PTR_START_NONE
    }

    debug_assert_eq!(w.pos, 32 + STARTS_IMAGE_BASE + STARTS_SEG_SIZE);

    // --- dyld_chained_import entries (4 bytes each) ---
    for (i, _name) in imports.iter().enumerate() {
        let name_off = name_offsets[i];
        // lib_ordinal=1, weak_import=0, name_offset in bits [31:9]
        let entry: u32 = 1 | (name_off << 9);
        w.u32(entry);
    }

    // --- symbol names ---
    w.bytes(&names);

    debug_assert_eq!(w.pos, total);

    ChainedFixups { blob, got_page_start }
}

/// Encode a dyld_chained_ptr_64_bind value for a GOT entry.
///
/// For DYLD_CHAINED_PTR_64 (stride=4 bytes):
/// - bit 63: bind=1
/// - bits [62:51]: next (in 4-byte stride units; 2 = 8 bytes to next entry)
/// - bits [50:32]: reserved=0
/// - bits [31:24]: addend=0
/// - bits [23:0]:  ordinal (index into import table)
pub fn got_bind_entry(ordinal: usize, is_last: bool) -> u64 {
    let next: u64 = if is_last { 0 } else { 2 }; // 2 * 4 = 8 bytes
    (1u64 << 63) | (next << 51) | (ordinal as u64 & 0x00FF_FFFF)
}

// ---------------------------------------------------------------------------
// Minimal byte writer
// ---------------------------------------------------------------------------

struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    fn new(buf: &'a mut [u8]) -> Self { Self { buf, pos: 0 } }
    fn u32(&mut self, v: u32) { self.buf[self.pos..self.pos+4].copy_from_slice(&v.to_le_bytes()); self.pos += 4; }
    fn u64(&mut self, v: u64) { self.buf[self.pos..self.pos+8].copy_from_slice(&v.to_le_bytes()); self.pos += 8; }
    fn u16(&mut self, v: u16) { self.buf[self.pos..self.pos+2].copy_from_slice(&v.to_le_bytes()); self.pos += 2; }
    fn bytes(&mut self, v: &[u8]) { self.buf[self.pos..self.pos+v.len()].copy_from_slice(v); self.pos += v.len(); }
}
