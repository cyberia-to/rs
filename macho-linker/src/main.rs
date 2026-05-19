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

use std::path::PathBuf;
use std::os::unix::fs::PermissionsExt;

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&raw) {
        Ok(a) => a,
        Err(e) => { eprintln!("macho-linker: {e}"); std::process::exit(1); }
    };

    if args.output.is_none() {
        eprintln!("macho-linker: no output file (-o)");
        std::process::exit(1);
    }

    match link(args) {
        Ok(()) => {}
        Err(e) => { eprintln!("macho-linker: {e}"); std::process::exit(1); }
    }
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

struct Args {
    inputs:     Vec<PathBuf>,
    output:     Option<PathBuf>,
    lib_paths:  Vec<PathBuf>,
    libs:       Vec<String>,
    frameworks: Vec<String>,
    entry:      String,
    dylibs:     Vec<String>, // resolved dylib paths to link against
    verbose:    bool,
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
        inputs:     Vec::new(),
        output:     None,
        lib_paths:  vec![
            PathBuf::from("/usr/lib"),
            PathBuf::from("/usr/local/lib"),
            sdk_lib,
        ],
        libs:       Vec::new(),
        frameworks: Vec::new(),
        entry:      "_main".to_string(),
        dylibs:     Vec::new(),
        verbose:    false,
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
            "-dynamic" | "-static" | "-pie" | "-no_pie" | "-dead_strip"
            | "-objc_abi_version" | "-bitcode_bundle" | "-ObjC"
            | "-lto_library" | "-no_adhoc_codesign" | "-adhoc_codesign" => {
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
            let install_name = extract_install_name(&path)
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
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
            if path.exists() { return Some(path); }
        }
        // Also try bare name (some libs are e.g. libSystem.B.dylib).
        let path = dir.join(format!("lib{name}.B.dylib"));
        if path.exists() { return Some(path); }
    }
    None
}

// ---------------------------------------------------------------------------
// Linker pipeline
// ---------------------------------------------------------------------------

fn link(args: Args) -> Result<(), String> {
    let output = args.output.as_ref().unwrap();

    // ---- Step 1: Parse inputs ----
    let mut objects: Vec<input::ObjData> = Vec::new();
    for path in &args.inputs {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "o" => {
                let mut obs = input::parse_file(path)?;
                objects.append(&mut obs);
            }
            "a" | "rlib" => {
                let mut obs = input::parse_file(path)?;
                objects.append(&mut obs);
            }
            _ => {
                // Try as object anyway (e.g., rlib renamed, or no extension).
                match input::parse_file(path) {
                    Ok(mut obs) => objects.append(&mut obs),
                    Err(e) => eprintln!("warning: skipping {}: {e}", path.display()),
                }
            }
        }
    }

    if objects.is_empty() {
        return Err("no input objects".to_string());
    }

    // ---- Step 2: Resolve symbols ----
    let mut syms = resolve::resolve(&objects, &args.dylibs)?;
    let imports = syms.imports.clone();
    let n_imports = imports.len();

    // ---- Step 3: Layout ----
    let mut layout = layout::perform(&objects, &mut syms, n_imports);

    // ---- Step 4: Apply relocations ----
    reloc::apply_all(&objects, &mut layout, &syms)?;

    // ---- Step 5: Emit ----
    let binary = emit::emit(&emit::EmitArgs {
        layout: &layout,
        syms: &syms,
        imports: &imports,
        dylibs: &args.dylibs,
        entry_symbol: &args.entry,
    });

    // ---- Step 6: Write output ----
    std::fs::write(output, &binary)
        .map_err(|e| format!("write {}: {e}", output.display()))?;
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(output, perms)
        .map_err(|e| format!("chmod {}: {e}", output.display()))?;

    if args.verbose {
        eprintln!("macho-linker: wrote {} ({} bytes)", output.display(), binary.len());
    }

    Ok(())
}
