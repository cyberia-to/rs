// pure-rust-check: Phase 0 audit tool for the pure-Rust toolchain project.
// Scans build artifacts for non-Rust inputs: sys-crates, cc/bindgen build
// scripts, and C-origin object files in target/.
//
// Usage:
//   pure-rust-check cargo-lock [path/to/Cargo.lock]
//   pure-rust-check build-scripts [path/to/project]
//   pure-rust-check artifacts [path/to/target]
//   pure-rust-check all [path/to/project]

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

fn warn(category: &str, msg: &str) {
    eprintln!("WARN: {category}: {msg}");
}

fn info(category: &str, msg: &str) {
    eprintln!("INFO: {category}: {msg}");
}

fn pass() {
    eprintln!("PASS: no issues found");
}

fn fail(n: usize) {
    eprintln!("FAIL: {n} issue{} found", if n == 1 { "" } else { "s" });
}

// ---------------------------------------------------------------------------
// Known non-pure-Rust crate names
// ---------------------------------------------------------------------------

const KNOWN_C_CRATES: &[&str] = &[
    "openssl",
    "libz-sys",
    "libgit2-sys",
    "libssh2-sys",
    "curl-sys",
    "sqlite3-sys",
    "zstd-sys",
    "bzip2-sys",
    "lzma-sys",
];

fn is_suspect_crate(name: &str) -> bool {
    if KNOWN_C_CRATES.contains(&name) {
        return true;
    }
    // Any crate ending in -sys is suspect
    name.ends_with("-sys")
}

// ---------------------------------------------------------------------------
// Cargo.lock scanner
// ---------------------------------------------------------------------------
//
// Cargo.lock (v3) looks like:
//
//   [[package]]
//   name = "foo"
//   version = "1.2.3"
//   ...
//
// We do a simple line-by-line scan — no external TOML crate.

struct PackageEntry {
    name: String,
    version: String,
}

fn parse_cargo_lock(text: &str) -> Vec<PackageEntry> {
    let mut entries: Vec<PackageEntry> = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_version: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed == "[[package]]" {
            // Flush previous entry if complete
            if let (Some(n), Some(v)) = (current_name.take(), current_version.take()) {
                entries.push(PackageEntry { name: n, version: v });
            } else {
                // Partial — drop
                current_name = None;
                current_version = None;
            }
            continue;
        }

        if let Some(value) = strip_key(trimmed, "name") {
            current_name = Some(value.to_string());
        } else if let Some(value) = strip_key(trimmed, "version") {
            current_version = Some(value.to_string());
        }
    }

    // Flush final entry
    if let (Some(n), Some(v)) = (current_name, current_version) {
        entries.push(PackageEntry { name: n, version: v });
    }

    entries
}

/// Parse `key = "value"` lines; return the unquoted value or None.
fn strip_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key} = \"");
    let stripped = line.strip_prefix(prefix.as_str())?;
    stripped.strip_suffix('"')
}

fn check_cargo_lock(lock_path: &Path) -> usize {
    let text = match fs::read_to_string(lock_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ERROR: cannot read {}: {e}", lock_path.display());
            return 1;
        }
    };

    let packages = parse_cargo_lock(&text);
    let mut issues = 0usize;

    for pkg in &packages {
        if is_suspect_crate(&pkg.name) {
            warn(
                "cargo-lock",
                &format!("{} {} (links to C)", pkg.name, pkg.version),
            );
            issues += 1;
        }
    }

    issues
}

// ---------------------------------------------------------------------------
// Build-script scanner
// ---------------------------------------------------------------------------

const BUILD_SCRIPT_PATTERNS: &[&str] = &[
    "cc::Build",
    "bindgen::",
    "pkg_config",
    r#"println!("cargo:rustc-link-lib="#,
    r#"Command::new("gcc""#,
    r#"Command::new("clang""#,
    r#"Command::new("cc""#,
];

fn scan_build_script(path: &Path) -> usize {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ERROR: cannot read {}: {e}", path.display());
            return 0;
        }
    };

    let mut issues = 0usize;

    for (lineno, line) in text.lines().enumerate() {
        let lineno_1 = lineno + 1;
        for pattern in BUILD_SCRIPT_PATTERNS {
            if line.contains(pattern) {
                warn(
                    "build-script",
                    &format!("{}:{lineno_1}: {pattern}", path.display()),
                );
                issues += 1;
                // Report each pattern once per line — no need to check others
                // for the same line to avoid duplicate output for the same hit.
                break;
            }
        }
    }

    issues
}

fn check_build_scripts(project_root: &Path) -> usize {
    let mut issues = 0usize;
    visit_files(project_root, &mut |path| {
        if path.file_name().map(|n| n == "build.rs").unwrap_or(false) {
            issues += scan_build_script(path);
        }
    });
    issues
}

// ---------------------------------------------------------------------------
// Artifact scanner
// ---------------------------------------------------------------------------

// Magic byte constants
const ELF_MAGIC: &[u8] = b"\x7fELF";
const MACHO32_MAGIC: &[u8] = b"\xCE\xFA\xED\xFE";
const MACHO64_MAGIC: &[u8] = b"\xCF\xFA\xED\xFE";
const AR_MAGIC: &[u8] = b"!<arch>\n";

// AR format constants
const AR_HEADER_SIZE: usize = 60;
const AR_NAME_SIZE: usize = 16;
const AR_SIZE_OFFSET: usize = 48;
const AR_SIZE_LEN: usize = 10;
const AR_FMAG_OFFSET: usize = 58;

#[derive(Debug, PartialEq)]
enum FileKind {
    Elf,
    MachO,
    Ar,
    Unknown,
}

fn detect_kind(header: &[u8]) -> FileKind {
    if header.len() >= 4 && &header[..4] == ELF_MAGIC {
        return FileKind::Elf;
    }
    if header.len() >= 4
        && (&header[..4] == MACHO32_MAGIC || &header[..4] == MACHO64_MAGIC)
    {
        return FileKind::MachO;
    }
    if header.len() >= 8 && &header[..8] == AR_MAGIC {
        return FileKind::Ar;
    }
    FileKind::Unknown
}

/// Scan an AR archive for member names ending in `.c` or `.c.o` (signs that
/// C sources were compiled into it).  Returns the names of suspicious members.
fn ar_c_members(data: &[u8]) -> Vec<String> {
    let mut suspicious: Vec<String> = Vec::new();
    if data.len() < 8 {
        return suspicious;
    }

    let mut pos = 8usize; // skip global header

    while pos + AR_HEADER_SIZE <= data.len() {
        let header = &data[pos..pos + AR_HEADER_SIZE];

        // Validate end-of-header marker (`\x60\x0a`)
        if &header[AR_FMAG_OFFSET..AR_FMAG_OFFSET + 2] != b"\x60\x0a" {
            break;
        }

        let raw_name = std::str::from_utf8(&header[..AR_NAME_SIZE])
            .unwrap_or("")
            .trim_end()
            .trim_end_matches('/');

        let raw_size = std::str::from_utf8(&header[AR_SIZE_OFFSET..AR_SIZE_OFFSET + AR_SIZE_LEN])
            .unwrap_or("0")
            .trim();

        let member_size: usize = raw_size.parse().unwrap_or(0);

        // Heuristic: member name contains ".c" suggests C compilation unit
        let name_lower = raw_name.to_ascii_lowercase();
        if name_lower.ends_with(".c")
            || name_lower.ends_with(".c.o")
            || name_lower.contains(".c.")
        {
            suspicious.push(raw_name.to_string());
        }

        // Advance past header + member data (padded to even boundary)
        pos += AR_HEADER_SIZE + member_size;
        if member_size % 2 != 0 {
            pos += 1;
        }
    }

    suspicious
}

fn check_artifact(path: &Path) -> usize {
    let mut buf = [0u8; 8];
    let n = match fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("ERROR: cannot read {}: {e}", path.display());
            return 0;
        }
    };

    let kind = detect_kind(&buf[..n]);

    match kind {
        FileKind::Elf | FileKind::MachO => {
            // A .o or .a with ELF/Mach-O magic is a native object.
            // We flag it as informational — could be from a C compiler.
            info(
                "artifacts",
                &format!(
                    "{}: native {} object (best-effort; verify with readelf/otool)",
                    path.display(),
                    if kind == FileKind::Elf { "ELF" } else { "Mach-O" }
                ),
            );
            1
        }
        FileKind::Ar => {
            // Read full file for AR member scan
            let data = match fs::read(path) {
                Ok(d) => d,
                Err(_) => return 0,
            };
            let c_members = ar_c_members(&data);
            if c_members.is_empty() {
                0
            } else {
                for m in &c_members {
                    info(
                        "artifacts",
                        &format!("{}: AR archive contains C member: {m}", path.display()),
                    );
                }
                c_members.len()
            }
        }
        FileKind::Unknown => 0,
    }
}

fn check_artifacts(target_dir: &Path) -> usize {
    let mut issues = 0usize;
    visit_files(target_dir, &mut |path| {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if ext == "a" || ext == "o" {
            issues += check_artifact(path);
        }
    });
    issues
}

// ---------------------------------------------------------------------------
// Directory walker
// ---------------------------------------------------------------------------

fn visit_files(dir: &Path, callback: &mut dyn FnMut(&Path)) {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.is_symlink() {
            continue; // skip symlinks to avoid cycles
        }

        if meta.is_dir() {
            visit_files(&path, callback);
        } else if meta.is_file() {
            callback(&path);
        }
    }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

fn resolve_path(arg: Option<&str>, default: &str) -> PathBuf {
    PathBuf::from(arg.unwrap_or(default))
}

fn print_usage() {
    eprintln!(
        "Usage:
  pure-rust-check cargo-lock [path/to/Cargo.lock]
  pure-rust-check build-scripts [path/to/project]
  pure-rust-check artifacts [path/to/target]
  pure-rust-check all [path/to/project]"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(2);
    }

    let subcommand = args[1].as_str();
    let path_arg = args.get(2).map(|s| s.as_str());

    let total_issues: usize = match subcommand {
        "cargo-lock" => {
            let lock = resolve_path(path_arg, "Cargo.lock");
            check_cargo_lock(&lock)
        }
        "build-scripts" => {
            let root = resolve_path(path_arg, ".");
            check_build_scripts(&root)
        }
        "artifacts" => {
            let target = resolve_path(path_arg, "target");
            check_artifacts(&target)
        }
        "all" => {
            let root = resolve_path(path_arg, ".");
            let lock = root.join("Cargo.lock");
            let target = root.join("target");

            let mut total = 0usize;
            total += check_cargo_lock(&lock);
            total += check_build_scripts(&root);
            total += check_artifacts(&target);
            total
        }
        "--help" | "-h" | "help" => {
            print_usage();
            process::exit(0);
        }
        other => {
            eprintln!("ERROR: unknown subcommand: {other}");
            print_usage();
            process::exit(2);
        }
    };

    if total_issues == 0 {
        pass();
        process::exit(0);
    } else {
        fail(total_issues);
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspect_crate_known() {
        assert!(is_suspect_crate("openssl"));
        assert!(is_suspect_crate("libz-sys"));
        assert!(is_suspect_crate("curl-sys"));
    }

    #[test]
    fn suspect_crate_sys_suffix() {
        assert!(is_suspect_crate("my-custom-sys"));
        assert!(is_suspect_crate("rocksdb-sys"));
    }

    #[test]
    fn innocent_crate() {
        assert!(!is_suspect_crate("serde"));
        assert!(!is_suspect_crate("tokio"));
        assert!(!is_suspect_crate("anyhow"));
    }

    #[test]
    fn parse_simple_lock() {
        let lock = r#"
[[package]]
name = "serde"
version = "1.0.0"

[[package]]
name = "openssl-sys"
version = "0.9.102"
"#;
        let pkgs = parse_cargo_lock(lock);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "serde");
        assert_eq!(pkgs[1].name, "openssl-sys");
        assert_eq!(pkgs[1].version, "0.9.102");
    }

    #[test]
    fn elf_magic_detection() {
        let data = b"\x7fELFrest";
        assert_eq!(detect_kind(data), FileKind::Elf);
    }

    #[test]
    fn macho_magic_detection() {
        let data = b"\xCF\xFA\xED\xFErest";
        assert_eq!(detect_kind(data), FileKind::MachO);
    }

    #[test]
    fn ar_magic_detection() {
        let data = b"!<arch>\nmore";
        assert_eq!(detect_kind(data), FileKind::Ar);
    }

    #[test]
    fn unknown_magic() {
        let data = b"\x00\x00\x00\x00";
        assert_eq!(detect_kind(data), FileKind::Unknown);
    }

    #[test]
    fn build_script_pattern_match() {
        let script = r#"
fn main() {
    cc::Build::new().file("src/foo.c").compile("foo");
}
"#;
        // Write to temp file and scan
        let dir = std::env::temp_dir();
        let path = dir.join("build_test.rs");
        fs::write(&path, script).unwrap();
        let issues = scan_build_script(&path);
        assert_eq!(issues, 1);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn build_script_clean() {
        let script = r#"
fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
}
"#;
        let dir = std::env::temp_dir();
        let path = dir.join("build_clean_test.rs");
        fs::write(&path, script).unwrap();
        let issues = scan_build_script(&path);
        assert_eq!(issues, 0);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn strip_key_parses_name() {
        assert_eq!(strip_key(r#"name = "foo""#, "name"), Some("foo"));
        assert_eq!(strip_key(r#"version = "1.2.3""#, "version"), Some("1.2.3"));
        assert_eq!(strip_key("something else", "name"), None);
    }

    #[test]
    fn io_error_returns_zero_for_missing_build_script() {
        let missing = Path::new("/nonexistent/build.rs");
        // Should not panic; returns 0 because read fails gracefully
        let issues = scan_build_script(missing);
        assert_eq!(issues, 0);
    }
}
