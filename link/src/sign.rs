//! Ad-hoc code signing for arm64 macOS Mach-O executables.
//!
//! Produces a CS_SuperBlob containing a CS_CodeDirectory with SHA-256
//! page hashes, suitable for LC_CODE_SIGNATURE on Apple Silicon.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const CS_MAGIC_EMBEDDED_SIGNATURE: u32 = 0xFADE_0CC0;
const CS_MAGIC_CODEDIRECTORY: u32 = 0xFADE_0C02;
const CS_ADHOC: u32 = 0x2;
const CS_LINKER_SIGNED: u32 = 0x0002_0000;
const CS_HASHTYPE_SHA256: u8 = 2;
const CD_VERSION: u32 = 0x0002_0400;
const CS_EXECSEG_MAIN_BINARY: u64 = 0x1;
const PAGE_SIZE: usize = 4096;

/// Fixed size of a CS_CodeDirectory header at version 0x20400.
/// magic(4)+length(4)+version(4)+flags(4)+hashOffset(4)+identOffset(4)+
/// nSpecialSlots(4)+nCodeSlots(4)+codeLimit(4)+
/// hashSize(1)+hashType(1)+platform(1)+pageSize(1)+
/// spare2(4)+scatterOffset(4)+teamOffset(4)+spare3(4)+
/// codeLimit64(8)+execSegBase(8)+execSegLimit(8)+execSegFlags(8)
const CD_HEADER_SIZE: usize = 88;

// ---------------------------------------------------------------------------
// Pure-Rust SHA-256 (no external dependencies)
// ---------------------------------------------------------------------------

fn sha256(data: &[u8]) -> [u8; 32] {
    #[rustfmt::skip]
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7)
                ^ w[i - 15].rotate_right(18)
                ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17)
                ^ w[i - 2].rotate_right(19)
                ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, &word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// Size computation (needed before building the binary so we can include
// LC_CODE_SIGNATURE + __LINKEDIT in the header layout pass)
// ---------------------------------------------------------------------------

/// Total bytes the signature blob will occupy for `code_limit` bytes of binary
/// content and the given identifier string.
pub fn signature_size(code_limit: usize, identifier: &str) -> usize {
    let ident_len = identifier.len() + 1; // null-terminated
    let n_pages = (code_limit + PAGE_SIZE - 1) / PAGE_SIZE;
    let cd_len = CD_HEADER_SIZE + ident_len + n_pages * 32;
    // SuperBlob: 12-byte header + 8-byte blob index entry + CD blob
    12 + 8 + cd_len
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Build a CS_SuperBlob containing a CS_CodeDirectory for `binary`.
///
/// `binary` is every byte of the Mach-O file up to but not including the
/// code signature data.  `exec_seg_limit` is `__TEXT.filesize` (the segment
/// boundary, NOT `binary.len()` which may include non-__TEXT LINKEDIT data).
///
/// All multi-byte fields in the blob are big-endian as required by the kernel.
pub fn make_code_signature(binary: &[u8], identifier: &str, exec_seg_limit: u64) -> Vec<u8> {
    let ident_bytes: Vec<u8> = {
        let mut v = identifier.as_bytes().to_vec();
        v.push(0);
        v
    };

    let n_pages = (binary.len() + PAGE_SIZE - 1) / PAGE_SIZE;
    let cd_header_size = CD_HEADER_SIZE as u32;
    let ident_offset = cd_header_size;
    let hash_offset = ident_offset + ident_bytes.len() as u32;
    let cd_length = hash_offset + n_pages as u32 * 32;

    // SuperBlob: 12-byte header + 8-byte index entry (1 blob) + CD blob
    let super_length = 12u32 + 8 + cd_length;
    // CD blob starts immediately after the 20-byte SuperBlob header+index
    let cd_offset_in_super: u32 = 20;

    let mut sig = Vec::with_capacity(super_length as usize);

    // --- CS_SuperBlob ---
    sig.extend_from_slice(&CS_MAGIC_EMBEDDED_SIGNATURE.to_be_bytes());
    sig.extend_from_slice(&super_length.to_be_bytes());
    sig.extend_from_slice(&1u32.to_be_bytes()); // count = 1 blob
    // blob index entry: type=0 (CSSLOT_CODEDIRECTORY), offset into super
    sig.extend_from_slice(&0u32.to_be_bytes());
    sig.extend_from_slice(&cd_offset_in_super.to_be_bytes());

    // --- CS_CodeDirectory ---
    let code_limit = binary.len() as u32;
    sig.extend_from_slice(&CS_MAGIC_CODEDIRECTORY.to_be_bytes());
    sig.extend_from_slice(&cd_length.to_be_bytes());
    sig.extend_from_slice(&CD_VERSION.to_be_bytes());
    sig.extend_from_slice(&(CS_ADHOC | CS_LINKER_SIGNED).to_be_bytes());
    sig.extend_from_slice(&hash_offset.to_be_bytes());
    sig.extend_from_slice(&ident_offset.to_be_bytes());
    sig.extend_from_slice(&0u32.to_be_bytes()); // nSpecialSlots
    sig.extend_from_slice(&(n_pages as u32).to_be_bytes());
    sig.extend_from_slice(&code_limit.to_be_bytes());
    sig.push(32u8); // hashSize (SHA-256 = 32 bytes)
    sig.push(CS_HASHTYPE_SHA256);
    sig.push(0); // platform
    sig.push(12); // pageSize = log2(4096)
    sig.extend_from_slice(&0u32.to_be_bytes()); // spare2
    sig.extend_from_slice(&0u32.to_be_bytes()); // scatterOffset
    sig.extend_from_slice(&0u32.to_be_bytes()); // teamOffset
    sig.extend_from_slice(&0u32.to_be_bytes()); // spare3
    sig.extend_from_slice(&0u64.to_be_bytes()); // codeLimit64 (unused when codeLimit fits u32)
    sig.extend_from_slice(&0u64.to_be_bytes()); // execSegBase = __TEXT fileoff = 0
    sig.extend_from_slice(&exec_seg_limit.to_be_bytes()); // execSegLimit = __TEXT filesize
    sig.extend_from_slice(&CS_EXECSEG_MAIN_BINARY.to_be_bytes()); // execSegFlags

    // Identifier string (null-terminated)
    sig.extend_from_slice(&ident_bytes);

    // Hash slots: SHA-256 of each page, last page hashed as-is (no zero padding)
    for page in 0..n_pages {
        let start = page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(binary.len());
        sig.extend_from_slice(&sha256(&binary[start..end]));
    }

    debug_assert_eq!(sig.len(), super_length as usize);
    sig
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_empty() {
        // SHA-256("") = e3b0c44298fc1c149afb...
        let h = sha256(b"");
        assert_eq!(h[0], 0xe3);
        assert_eq!(h[1], 0xb0);
        assert_eq!(h[2], 0xc4);
        assert_eq!(h[3], 0x42);
    }

    #[test]
    fn sha256_abc() {
        // SHA-256("abc") = ba7816bf8f01cfea4141...
        let h = sha256(b"abc");
        assert_eq!(h[0], 0xba);
        assert_eq!(h[1], 0x78);
        assert_eq!(h[2], 0x16);
        assert_eq!(h[3], 0xbf);
    }

    #[test]
    fn signature_starts_with_superblob_magic() {
        let binary = vec![0u8; 64]; // tiny dummy binary
        let sig = make_code_signature(&binary, "test", 64);
        // CS_MAGIC_EMBEDDED_SIGNATURE = 0xFADE0CC0 big-endian
        assert_eq!(&sig[0..4], &[0xFA, 0xDE, 0x0C, 0xC0]);
    }

    #[test]
    fn signature_size_matches() {
        let binary = vec![0u8; 8000]; // 2 pages
        let expected = signature_size(binary.len(), "test");
        let actual = make_code_signature(&binary, "test", 8000).len();
        assert_eq!(actual, expected);
    }
}
