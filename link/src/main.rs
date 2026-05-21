use link::{EmitInput, StaticData, DataReloc, emit_macho};
use std::os::unix::fs::PermissionsExt;

fn main() {
    let mut code: Vec<u8> = Vec::new();
    code.extend_from_slice(&0x90000001u32.to_le_bytes()); // ADRP x1, 0
    code.extend_from_slice(&0x91000021u32.to_le_bytes()); // ADD x1, x1, #0
    code.extend_from_slice(&0xD2800020u32.to_le_bytes()); // MOVZ x0, #1
    code.extend_from_slice(&0xD28001C2u32.to_le_bytes()); // MOVZ x2, #14
    code.extend_from_slice(&0xD2800090u32.to_le_bytes()); // MOVZ x16, #4
    code.extend_from_slice(&0xD4001001u32.to_le_bytes()); // SVC #0x80
    code.extend_from_slice(&0xD2800000u32.to_le_bytes()); // MOVZ x0, #0
    code.extend_from_slice(&0xD2800030u32.to_le_bytes()); // MOVZ x16, #1
    code.extend_from_slice(&0xD4001001u32.to_le_bytes()); // SVC #0x80

    let input = EmitInput {
        code: &code,
        data: vec![StaticData { name: "__msg".into(), bytes: b"Hello, world!\n".to_vec(), writable: false }],
        relocs: vec![DataReloc { adrp_offset: 0, add_offset: 4, symbol: "__msg".into() }],
        static_relocs: vec![],
        fn_offsets: std::collections::HashMap::new(),
        entry_offset: 0,
    };
    let binary = emit_macho(&input);
    let path = "/tmp/hello_pure_rs";
    std::fs::write(path, &binary).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    println!("wrote {} bytes to {}", binary.len(), path);
}
