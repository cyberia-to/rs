//! Parse Mach-O MH_OBJECT files and ar archives into `ObjData`.

use object::{
    Object, ObjectSection, ObjectSymbol, RelocationTarget,
    SymbolKind, RelocationFlags,
};
use object::read::archive::ArchiveFile;

/// Raw ARM64 Mach-O relocation types (from mach-o/arm64/reloc.h).
mod arm64_reloc {
    pub const UNSIGNED:           u8 = 0;
    pub const SUBTRACTOR:         u8 = 1;
    pub const BRANCH26:           u8 = 2;
    pub const PAGE21:             u8 = 3;
    pub const PAGEOFF12:          u8 = 4;
    pub const GOT_LOAD_PAGE21:    u8 = 5;
    pub const GOT_LOAD_PAGEOFF12: u8 = 6;
    pub const POINTER_TO_GOT:     u8 = 7;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RelocKind {
    Unsigned,
    Branch26,
    Page21,
    PageOff12,
    GotPage21,
    GotPageOff12,
    PtrToGot,
    Subtractor,
}

#[derive(Debug, Clone)]
pub enum RelocTarget {
    Symbol(String),
    SectionRel { seg: String, sec: String },
}

#[derive(Debug, Clone)]
pub struct Reloc {
    pub offset: u64,
    pub target: RelocTarget,
    pub kind: RelocKind,
    pub addend: i64,
    pub size_bits: u8,
}

#[derive(Debug, Clone)]
pub struct SecData {
    pub seg: String,
    pub name: String,
    pub data: Vec<u8>,
    pub align: u32,  // log2 alignment
    pub flags: u32,
    pub relocs: Vec<Reloc>,
}

#[derive(Debug, Clone)]
pub struct SymData {
    pub name: String,
    pub is_defined: bool,
    pub is_global: bool,
    pub section_idx: Option<usize>,
    pub offset: u64,
}

#[derive(Debug)]
pub struct ObjData {
    pub source: String,
    pub sections: Vec<SecData>,
    pub symbols: Vec<SymData>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn parse_file(path: &std::path::Path) -> Result<Vec<ObjData>, String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    parse_bytes(&data, &path.to_string_lossy())
}

pub fn parse_bytes(data: &[u8], name: &str) -> Result<Vec<ObjData>, String> {
    if data.starts_with(b"!<arch>\n") {
        return parse_archive(data, name);
    }
    Ok(vec![parse_object(data, name)?])
}

// ---------------------------------------------------------------------------
// Archive parsing
// ---------------------------------------------------------------------------

fn parse_archive(data: &[u8], archive_name: &str) -> Result<Vec<ObjData>, String> {
    let archive = ArchiveFile::parse(data)
        .map_err(|e| format!("parse archive {archive_name}: {e}"))?;
    let mut out = Vec::new();
    for member in archive.members() {
        let member = member.map_err(|e| format!("archive member in {archive_name}: {e}"))?;
        let member_name = String::from_utf8_lossy(member.name()).into_owned();
        let member_data = member.data(data)
            .map_err(|e| format!("archive member data {member_name}: {e}"))?;
        if is_macho(member_data) {
            let label = format!("{archive_name}({member_name})");
            match parse_object(member_data, &label) {
                Ok(obj) => out.push(obj),
                Err(e) => eprintln!("warning: skipping {label}: {e}"),
            }
        }
    }
    Ok(out)
}

fn is_macho(data: &[u8]) -> bool {
    if data.len() < 4 { return false; }
    let magic = u32::from_le_bytes(data[0..4].try_into().unwrap());
    magic == 0xFEED_FACF || magic == 0xCEFA_EDFE
}

// ---------------------------------------------------------------------------
// Single object file parsing
// ---------------------------------------------------------------------------

fn parse_object(data: &[u8], name: &str) -> Result<ObjData, String> {
    let file = object::File::parse(data)
        .map_err(|e| format!("parse {name}: {e}"))?;

    let sections = parse_sections(&file, data)?;
    let symbols  = parse_symbols(&file, &sections)?;

    Ok(ObjData { source: name.to_string(), sections, symbols })
}

fn seg_name_of(sec: &object::Section<'_, '_>) -> String {
    sec.segment_name().ok().flatten().unwrap_or("").to_string()
}

fn parse_sections(file: &object::File<'_>, raw: &[u8]) -> Result<Vec<SecData>, String> {
    let mut out = Vec::new();
    for sec in file.sections() {
        let name = sec.name().unwrap_or("").to_string();
        let seg  = seg_name_of(&sec);

        // Skip debug sections for now.
        if seg == "__DWARF" { continue; }

        let data = sec.data().unwrap_or(&[]).to_vec();
        let byte_align = sec.align() as usize;
        let log2_align = if byte_align <= 1 { 0 } else { byte_align.trailing_zeros() };

        // Mach-O section flags from raw bytes.
        let flags = sec_flags(file, &sec);

        let relocs = parse_relocs(file, &sec, raw, &out, file.sections().count())?;
        out.push(SecData { seg, name, data, align: log2_align, flags, relocs });
    }
    Ok(out)
}

fn sec_flags(_file: &object::File<'_>, sec: &object::Section<'_, '_>) -> u32 {
    use object::SectionKind;
    match sec.kind() {
        SectionKind::ReadOnlyString  => 0x02, // S_CSTRING_LITERALS
        SectionKind::UninitializedData => 0x01, // S_ZEROFILL
        _ => 0,
    }
}

fn parse_relocs(
    file: &object::File<'_>,
    sec: &object::Section<'_, '_>,
    raw_file: &[u8],
    sections_so_far: &[SecData],
    _total_sections: usize,
) -> Result<Vec<Reloc>, String> {
    let mut out = Vec::new();
    let sec_raw = sec.data().unwrap_or(&[]);

    for (offset, reloc) in sec.relocations() {
        let (r_type, r_length) = match reloc.flags() {
            RelocationFlags::MachO { r_type, r_length, .. } => (r_type, r_length),
            _ => continue,
        };

        let kind = match r_type {
            arm64_reloc::UNSIGNED           => RelocKind::Unsigned,
            arm64_reloc::SUBTRACTOR         => RelocKind::Subtractor,
            arm64_reloc::BRANCH26           => RelocKind::Branch26,
            arm64_reloc::PAGE21             => RelocKind::Page21,
            arm64_reloc::PAGEOFF12          => RelocKind::PageOff12,
            arm64_reloc::GOT_LOAD_PAGE21    => RelocKind::GotPage21,
            arm64_reloc::GOT_LOAD_PAGEOFF12 => RelocKind::GotPageOff12,
            arm64_reloc::POINTER_TO_GOT     => RelocKind::PtrToGot,
            other => {
                eprintln!("warning: unhandled ARM64 reloc type {} at {offset:#x}, skipping", other);
                continue;
            }
        };

        let size_bits: u8 = match r_length {
            0 => 8,
            1 => 16,
            2 => 32,
            _ => 64,
        };

        // r_extern is implicit: Symbol target → extern, Section target → section-relative.
        let target = match reloc.target() {
            RelocationTarget::Symbol(idx) => {
                let sym = file.symbol_by_index(idx)
                    .map_err(|e| format!("symbol {idx:?}: {e}"))?;
                RelocTarget::Symbol(sym.name().unwrap_or("").to_string())
            }
            RelocationTarget::Section(idx) => {
                let s = file.section_by_index(idx)
                    .map_err(|e| format!("section {idx:?}: {e}"))?;
                RelocTarget::SectionRel {
                    seg: seg_name_of(&s),
                    sec: s.name().unwrap_or("").to_string(),
                }
            }
            _ => RelocTarget::Symbol(String::new()),
        };

        // Read the implicit addend from the instruction/data bytes.
        let addend: i64 = read_addend(sec_raw, offset as usize, kind, size_bits);

        out.push(Reloc { offset, target, kind, addend, size_bits });
    }
    Ok(out)
}

/// Read the embedded addend from the relocation site.
/// For instruction relocations (BRANCH26, PAGE21, etc.) the field is 0 in .o files.
/// For UNSIGNED, the full address is at the site.
fn read_addend(data: &[u8], off: usize, kind: RelocKind, size_bits: u8) -> i64 {
    match kind {
        RelocKind::Unsigned => {
            if size_bits == 64 && off + 8 <= data.len() {
                i64::from_le_bytes(data[off..off + 8].try_into().unwrap())
            } else if off + 4 <= data.len() {
                i32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as i64
            } else {
                0
            }
        }
        // Instruction relocations: the field in the .o is 0.
        _ => 0,
    }
}

fn parse_symbols(file: &object::File<'_>, sections: &[SecData]) -> Result<Vec<SymData>, String> {
    let mut out = Vec::new();
    for sym in file.symbols() {
        let name = sym.name().unwrap_or("").to_string();
        if name.is_empty() { continue; }

        let kind = sym.kind();
        // Keep Unknown: it's Mach-O's N_UNDF — undefined externals (_write, _exit etc.).
        if matches!(kind, SymbolKind::Section | SymbolKind::File) { continue; }

        let is_defined = sym.is_definition();
        let is_global  = matches!(
            sym.scope(),
            object::SymbolScope::Linkage | object::SymbolScope::Dynamic
        );

        let (section_idx, offset) = if is_defined {
            match sym.section() {
                object::SymbolSection::Section(sec_idx) => {
                    let idx = find_section_idx(file, sec_idx, sections);
                    // sym.address() is absolute in the .o layout; make it section-relative.
                    let sec_base = file.section_by_index(sec_idx)
                        .map(|s| s.address())
                        .unwrap_or(0);
                    (idx, sym.address().saturating_sub(sec_base))
                }
                _ => (None, sym.address()),
            }
        } else {
            (None, 0)
        };

        out.push(SymData { name, is_defined, is_global, section_idx, offset });
    }
    Ok(out)
}

fn find_section_idx(
    file: &object::File<'_>,
    sec_idx: object::SectionIndex,
    sections: &[SecData],
) -> Option<usize> {
    let sec = file.section_by_index(sec_idx).ok()?;
    let seg = seg_name_of(&sec);
    let name = sec.name().ok()?.to_string();
    // Find matching section by seg+name. Since DWARF is filtered, simple scan.
    sections.iter().position(|s| s.seg == seg && s.name == name)
}
