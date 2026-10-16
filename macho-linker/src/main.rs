//! macho-linker: pure-Rust Mach-O multi-object linker for arm64 macOS.
//!
//! CLI is ld64-compatible (subset): accepts .o files, .a archives, -o, -l, -L,
//! -framework, -arch, -platform_version, -rpath, -e.

mod arm64;
mod dyld;
mod emit;
mod input;
mod layout;
mod reloc;
mod resolve;
mod tlv;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("macho-linker: {e}");
            std::process::exit(1);
        }
    };

    if args.output.is_none() {
        eprintln!("macho-linker: no output file (-o)");
        std::process::exit(1);
    }

    match link(args) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("macho-linker: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

struct Args {
    inputs: Vec<PathBuf>,
    output: Option<PathBuf>,
    lib_paths: Vec<PathBuf>,
    libs: Vec<String>,
    frameworks: Vec<String>,
    entry: String,
    dylibs: Vec<String>, // resolved dylib paths to link against
    verbose: bool,
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    // Include SDK lib path for .tbd stubs (macOS shared-cache era).
    let sdk_lib = std::process::Command::new("xcrun")
        .args(["--show-sdk-path"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| PathBuf::from(s.trim()).join("usr/lib"))
        .unwrap_or_default();

    let mut args = Args {
        inputs: Vec::new(),
        output: None,
        lib_paths: vec![
            PathBuf::from("/usr/lib"),
            PathBuf::from("/usr/local/lib"),
            sdk_lib,
        ],
        libs: Vec::new(),
        frameworks: Vec::new(),
        entry: "_main".to_string(),
        dylibs: Vec::new(),
        verbose: false,
    };

    let mut i = 0;
    while i < raw.len() {
        let arg = &raw[i];
        match arg.as_str() {
            "-o" => {
                i += 1;
                args.output = Some(PathBuf::from(&raw[i]));
            }
            "-e" => {
                i += 1;
                args.entry = raw[i].clone();
            }
            "-L" => {
                i += 1;
                args.lib_paths.push(PathBuf::from(&raw[i]));
            }
            "-framework" => {
                i += 1;
                args.frameworks.push(raw[i].clone());
                // Add framework dylib path.
                let fw = &raw[i];
                let path = format!("/System/Library/Frameworks/{fw}.framework/{fw}");
                if PathBuf::from(&path).exists() {
                    args.dylibs.push(path);
                }
            }
            "-rpath" | "-map" | "-demangle" | "-no_deduplicate" | "-reproducible" => {
                i += 1; // skip argument
            }
            "-arch" | "-platform_version" | "-sdk_version" | "-deployment_target" => {
                i += 1; // skip value
                if arg == "-platform_version" {
                    i += 2; // platform_version takes 3 args total: name minOS sdk
                }
            }
            "-syslibroot" | "-install_name" | "-final_output" => {
                i += 1; // skip value
            }
            "-dynamic" | "-static" | "-pie" | "-no_pie" | "-dead_strip" | "-objc_abi_version"
            | "-bitcode_bundle" | "-ObjC" | "-lto_library" | "-no_adhoc_codesign"
            | "-adhoc_codesign" => {
                // flags with no separate value
            }
            "-v" | "--version" | "-version_details" => {
                args.verbose = true;
            }
            s if s.starts_with("-L") => {
                args.lib_paths.push(PathBuf::from(&s[2..]));
            }
            s if s.starts_with("-l") => {
                args.libs.push(s[2..].to_string());
            }
            s if s.starts_with("-") => {
                // Unknown flag — ignore silently (ld64 compatibility).
            }
            path => {
                let p = PathBuf::from(path);
                if p.exists() {
                    args.inputs.push(p);
                } else {
                    eprintln!("warning: input not found: {path}");
                }
            }
        }
        i += 1;
    }

    // Resolve -l libraries.
    for lib in &args.libs {
        if let Some(path) = find_lib(lib, &args.lib_paths) {
            let install_name =
                extract_install_name(&path).unwrap_or_else(|| path.to_string_lossy().into_owned());
            args.dylibs.push(install_name);
        } else {
            eprintln!("warning: library not found: -l{lib}");
        }
    }

    Ok(args)
}

/// Extract the dylib install name from a .tbd stub or .dylib.
/// TBD files have `install-name: '/usr/lib/libFoo.dylib'` near the top.
/// For .dylib files we fall back to the path itself (LC_ID_DYLIB parsing omitted).
fn extract_install_name(path: &std::path::Path) -> Option<String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == "tbd" {
        let content = std::fs::read_to_string(path).ok()?;
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("install-name:") {
                let name = rest.trim().trim_matches('\'').trim_matches('"');
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
        None
    } else {
        None
    }
}

fn find_lib(name: &str, search: &[PathBuf]) -> Option<PathBuf> {
    for dir in search {
        // Try dylib first, then static (.a).
        for suffix in &[".dylib", ".tbd", ".a"] {
            let path = dir.join(format!("lib{name}{suffix}"));
            if path.exists() {
                return Some(path);
            }
        }
        // Also try bare name (some libs are e.g. libSystem.B.dylib).
        let path = dir.join(format!("lib{name}.B.dylib"));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Linker pipeline
// ---------------------------------------------------------------------------

fn link(args: Args) -> Result<(), String> {
    let output = args.output.as_ref().unwrap();

    // ---- Step 1: Parse inputs, keeping archive boundaries ----
    let mut objects: Vec<input::ObjData> = Vec::new();
    let mut archives: Vec<Vec<input::ObjData>> = Vec::new();
    for path in &args.inputs {
        let data = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        if data.starts_with(b"!<arch>\n") {
            archives.push(input::parse_bytes(&data, &path.to_string_lossy())?);
        } else {
            objects.push(input::parse_object(&data, &path.to_string_lossy())?);
        }
    }

    if objects.is_empty() && archives.is_empty() {
        return Err("no input objects".to_string());
    }

    // ---- Step 2: Demand-driven archive member inclusion ----
    // Seed with symbols undefined in plain objects (plus the entry symbol).
    // A member is pulled only if it defines a currently-undefined symbol;
    // pulling it contributes its own undefined symbols to the pool. Loop to
    // a fixpoint. This mirrors archive semantics of ld/ld64 and avoids
    // dragging in (and failing on) unreferenced members.
    let mut defined: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut undefined: std::collections::HashSet<String> = std::collections::HashSet::new();
    for obj in &objects {
        for sym in &obj.symbols {
            if sym.is_defined {
                defined.insert(sym.name.clone());
            }
        }
    }
    for obj in &objects {
        for sym in &obj.symbols {
            if !sym.is_defined && !defined.contains(&sym.name) {
                undefined.insert(sym.name.clone());
            }
        }
    }
    undefined.insert(args.entry.clone());

    let mut taken: Vec<Vec<bool>> = archives.iter().map(|m| vec![false; m.len()]).collect();
    loop {
        let mut progress = false;
        for (ai, members) in archives.iter().enumerate() {
            for (mi, member) in members.iter().enumerate() {
                if taken[ai][mi] {
                    continue;
                }
                let pulls: Vec<String> = member
                    .symbols
                    .iter()
                    .filter(|s| s.is_defined && undefined.contains(&s.name))
                    .map(|s| s.name.clone())
                    .collect();
                if pulls.is_empty() {
                    continue;
                }
                taken[ai][mi] = true;
                progress = true;
                for name in pulls {
                    undefined.remove(&name);
                }
                for sym in &member.symbols {
                    if sym.is_defined {
                        defined.insert(sym.name.clone());
                    } else if !defined.contains(&sym.name) {
                        undefined.insert(sym.name.clone());
                    }
                }
            }
        }
        if !progress {
            break;
        }
    }

    for (ai, members) in archives.into_iter().enumerate() {
        for (mi, member) in members.into_iter().enumerate() {
            if taken[ai][mi] {
                objects.push(member);
            }
        }
    }

    // ---- Step 3: Resolve symbols ----
    let mut syms = resolve::resolve(&objects, &args.dylibs)?;
    resolve::collect_local_gots(&objects, &mut syms);
    let imports = syms.imports.clone();
    let n_imports = imports.len();
    let n_got = n_imports + syms.local_gots.len();

    // ---- Step 4: Layout ----
    let mut layout = layout::perform(&objects, &mut syms, n_imports, n_got);

    // ---- Step 5: Apply relocations ----
    reloc::apply_all(&objects, &mut layout, &syms)?;

    // ---- Step 5b: Finalize TLV descriptors (needs resolved $tlv$init addrs) ----
    let has_tlv = tlv::finalize_tlv(&mut layout, &objects)?;

    // ---- Step 5c: Fill local GOT slots with their symbols' absolute
    // addresses (imports stay zero — dyld binds them via the fixup chain).
    // Every GOT slot becomes a fixup entry: imports Bind, locals Rebase.
    if let Some(g_idx) = layout.got_idx {
        for (pos, name) in syms.local_gots.iter().enumerate() {
            if let Some(gs) = syms.syms.get(name) {
                let idx = n_imports + pos;
                let off = idx * 8;
                layout.merged[g_idx].data[off..off + 8].copy_from_slice(&gs.addr.to_le_bytes());
                let slot_vm = layout.merged[g_idx].vm_addr + off as u64;
                layout.fixup_slots.push(dyld::FixupSlot::Rebase {
                    vm_addr: slot_vm,
                    target: gs.addr,
                });
            }
        }
        for (ordinal, _) in imports.iter().enumerate() {
            let off = ordinal * 8;
            let slot_vm = layout.merged[g_idx].vm_addr + off as u64;
            layout.fixup_slots.push(dyld::FixupSlot::Bind {
                vm_addr: slot_vm,
                ordinal,
            });
        }
    }

    // ---- Step 6: Emit ----
    // De-duplicate slots by vm address once, so the blob chains and the
    // on-disk entries cover exactly the same slot set.
    {
        let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
        layout.fixup_slots.retain(|s| seen.insert(s.vm_addr()));
    }
    let binary = emit::emit(&emit::EmitArgs {
        layout: &layout,
        syms: &syms,
        imports: &imports,
        dylibs: &args.dylibs,
        entry_symbol: &args.entry,
        has_tlv,
    });

    // ---- Step 6: Write output ----
    std::fs::write(output, &binary).map_err(|e| format!("write {}: {e}", output.display()))?;
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(output, perms)
        .map_err(|e| format!("chmod {}: {e}", output.display()))?;

    if args.verbose {
        eprintln!(
            "macho-linker: wrote {} ({} bytes)",
            output.display(),
            binary.len()
        );
    }

    Ok(())
}
