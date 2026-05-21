//! Symbol resolution: build a global symbol table from all input objects.

use std::collections::HashMap;

use crate::input::ObjData;

/// Resolved symbol in the final output.
#[derive(Debug, Clone)]
pub struct GlobalSym {
    /// Final VM address (assigned during layout).
    pub addr: u64,
    pub kind: SymKind,
}

#[derive(Debug, Clone)]
pub enum SymKind {
    /// Defined in one of the input objects.
    Defined {
        obj_idx: usize,
        sec_idx: usize, // index into ObjData.sections
        offset: u64,    // byte offset within section
    },
    /// External symbol from a dylib. addr = stub address (set during layout).
    DylibImport {
        dylib_ordinal: u16, // 1-based LC_LOAD_DYLIB ordinal
        got_idx: usize,     // index into GOT table
        stub_idx: usize,    // index into stubs section
    },
}

/// Outcome of symbol resolution.
pub struct SymbolTable {
    pub syms: HashMap<String, GlobalSym>,
    /// External symbols that need dylib binding, in GOT/stub order.
    pub imports: Vec<String>,
}

pub fn resolve(
    objects: &[ObjData],
    dylib_names: &[String], // dylib names we provide (e.g., "libSystem.B.dylib")
) -> Result<SymbolTable, String> {
    let mut table: HashMap<String, GlobalSym> = HashMap::new();
    let mut undefined: Vec<String> = Vec::new();

    // Pass 1: collect all defined symbols (global and local).
    for (obj_idx, obj) in objects.iter().enumerate() {
        for sym in &obj.symbols {
            if !sym.is_defined { continue; }
            let sec_idx = match sym.section_idx {
                Some(i) => i,
                None => continue, // absolute symbol; skip for now
            };
            let entry = GlobalSym {
                addr: 0, // filled in by layout
                kind: SymKind::Defined { obj_idx, sec_idx, offset: sym.offset },
            };
            if let Some(prev) = table.insert(sym.name.clone(), entry) {
                // Duplicate definition: keep the first, warn.
                if let SymKind::Defined { obj_idx: prev_obj, .. } = prev.kind {
                    eprintln!(
                        "warning: duplicate symbol '{}' in {} and {}; keeping first",
                        sym.name, objects[prev_obj].source, obj.source
                    );
                    // Re-insert the original.
                    table.insert(sym.name.clone(), prev);
                }
            }
        }
    }

    // Pass 2: collect undefined symbols from all objects.
    let mut seen_undef: std::collections::HashSet<String> = std::collections::HashSet::new();
    for obj in objects {
        for sym in &obj.symbols {
            if sym.is_defined { continue; }
            if table.contains_key(&sym.name) { continue; }
            if seen_undef.insert(sym.name.clone()) {
                undefined.push(sym.name.clone());
            }
        }
    }

    // Pass 3: assign dylib imports for remaining undefined symbols.
    // For now all unknown symbols come from dylib ordinal 1 (libSystem or whichever
    // dylib the user specified).
    let mut imports: Vec<String> = Vec::new();
    for sym_name in &undefined {
        let got_idx = imports.len();
        let stub_idx = imports.len();
        imports.push(sym_name.clone());
        table.insert(sym_name.clone(), GlobalSym {
            addr: 0, // stub address assigned during layout
            kind: SymKind::DylibImport { dylib_ordinal: 1, got_idx, stub_idx },
        });
    }

    Ok(SymbolTable { syms: table, imports })
}
