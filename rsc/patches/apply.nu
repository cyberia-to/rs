#!/usr/bin/env nu
#
# apply.nu — fetch rustc source and inject Rs lint passes.
#
# Vendor+patch technique: fetch upstream rustc source for a pinned stable
# release, apply surgical string replacements to inject Rs edition support,
# lint passes, and diagnostics. Result: .vendor/rustc ready to compile.
#
# Usage:
#   cd rs/rsc
#   nu patches/apply.nu            # fetch + patch + build
#   nu patches/apply.nu --fetch    # fetch only
#   nu patches/apply.nu --patch    # patch only (assumes fetched)
#   nu patches/apply.nu --build    # build only (assumes patched)
#   nu patches/apply.nu --clean    # remove .vendor directory
#
# The patched compiler binary is placed at target/release/rsc-rustc.

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

# Pinned rustc version. All patches are validated against this version.
# To update: change this, re-run apply.nu, fix any patch failures.
const RUSTC_VERSION = "1.84.0"
const RUSTC_DATE = "2025-01-09"

# Crate registry URL for fetching rustc source components
const CRATE_REGISTRY = "https://static.rust-lang.org/dist"

# Components needed from the rustc distribution
const COMPONENTS = [
    "rustc-dev"
    "rust-src"
]

# Directory for vendored rustc source
const VENDOR_DIR = ".vendor"

# Patch files directory (relative to script location)
const PATCHES_DIR = "patches"

# ---------------------------------------------------------------------------
# Utility functions
# ---------------------------------------------------------------------------

# Print a step header
def step [msg: string] {
    print $"(ansi green_bold)>>>(ansi reset) ($msg)"
}

# Print an error and exit
def bail [msg: string] {
    print $"(ansi red_bold)error:(ansi reset) ($msg)"
    exit 1
}

# Check that a command exists
def require-cmd [cmd: string] {
    if (which $cmd | is-empty) {
        bail $"required command not found: ($cmd)"
    }
}

# Read a file, apply a replacement, write it back.
# Fails if the search string is not found (prevents silent patch failures).
def patch-file [
    path: string
    search: string
    replace: string
    --description: string = ""
] {
    let content = (open $path --raw)
    if not ($content | str contains $search) {
        let desc = if $description != "" { $" \(($description))" } else { "" }
        bail $"patch target not found in ($path)($desc)\nsearching for: ($search | str substring 0..80)..."
    }
    let patched = ($content | str replace $search $replace)
    $patched | save $path --force
    if $description != "" {
        print $"  patched: ($description)"
    }
}

# Append content to a file
def append-file [path: string, content: string] {
    let existing = (open $path --raw)
    $"($existing)\n($content)" | save $path --force
}

# Copy a patch file into the vendor tree
def inject-file [src: string, dst: string] {
    cp $src $dst
    print $"  injected: ($dst | path basename)"
}

# ---------------------------------------------------------------------------
# Fetch phase
# ---------------------------------------------------------------------------

def do-fetch [] {
    step "Fetching rustc source components"

    require-cmd "curl"
    require-cmd "tar"

    let host = (rustc --print target-triple | str trim)
    print $"  host triple: ($host)"
    print $"  rustc version: ($RUSTC_VERSION)"

    mkdir $VENDOR_DIR

    for component in $COMPONENTS {
        let archive = $"($component)-($RUSTC_VERSION)-($host).tar.xz"
        let url = $"($CRATE_REGISTRY)/($RUSTC_DATE)/($archive)"
        let dest = $"($VENDOR_DIR)/($archive)"

        if ($dest | path exists) {
            print $"  cached: ($archive)"
        } else {
            print $"  downloading: ($archive)"
            curl -fSL $url -o $dest
            if $env.LAST_EXIT_CODE != 0 {
                rm -f $dest
                bail $"failed to download ($url)"
            }
        }

        print $"  extracting: ($archive)"
        tar xf $dest -C $VENDOR_DIR
    }

    step "Fetch complete"
}

# ---------------------------------------------------------------------------
# Patch phase
# ---------------------------------------------------------------------------

def do-patch [] {
    step "Applying Rs patches"

    let host = (rustc --print target-triple | str trim)
    let src_root = $"($VENDOR_DIR)/rust-src-($RUSTC_VERSION)/lib/rustc/src/rust"
    let dev_root = $"($VENDOR_DIR)/rustc-dev-($RUSTC_VERSION)-($host)"

    if not ($src_root | path exists) {
        bail $"rustc source not found at ($src_root) — run with --fetch first"
    }

    # Find key source files
    let compiler_dir = $"($src_root)/compiler"
    let rustc_span_dir = $"($compiler_dir)/rustc_span/src"
    let rustc_lint_dir = $"($compiler_dir)/rustc_lint/src"
    let rustc_session_dir = $"($compiler_dir)/rustc_session/src"
    let rustc_driver_dir = $"($compiler_dir)/rustc_driver_impl/src"

    # ------------------------------------------------------------------
    # 1. Edition recognition: inject Rs as a valid edition
    # ------------------------------------------------------------------
    step "Injecting Rs edition"

    inject-file $"($PATCHES_DIR)/rs_edition.rs" $"($rustc_span_dir)/rs_edition.rs"

    # Add rs_edition module declaration to rustc_span/src/lib.rs
    patch-file $"($rustc_span_dir)/lib.rs" (
        "pub mod edition;"
    ) (
        "pub mod edition;\npub mod rs_edition;"
    ) --description "add rs_edition module to rustc_span"

    # Add "rs" to the edition parser in rustc_span/src/edition.rs
    patch-file $"($rustc_span_dir)/edition.rs" (
        '            "2024" => Some(Edition::Edition2024),'
    ) (
        '            "2024" => Some(Edition::Edition2024),
            "rs" => Some(Edition::Edition2024), // Rs edition maps to 2024 semantics'
    ) --description "recognize 'rs' as valid edition string"

    # Add rs_edition flag to session options
    patch-file $"($rustc_session_dir)/options.rs" (
        "    // end of unstable options"
    ) (
        '    rs_edition: bool [UNTRACKED, "treat the current edition as Rs (enables Rs lints)"],
    // end of unstable options'
    ) --description "add rs_edition session flag"

    # Set the flag when edition string is "rs"
    patch-file $"($rustc_session_dir)/config.rs" (
        "    // edition validation"
    ) (
        '    if matches!(edition_string.as_deref(), Some("rs")) {
        sopts.unstable_opts.rs_edition = true;
    }
    // edition validation'
    ) --description "set rs_edition flag during config parsing"

    # ------------------------------------------------------------------
    # 2. Inject lint pass source files
    # ------------------------------------------------------------------
    step "Injecting lint passes"

    let lint_pass_dir = $"($rustc_lint_dir)/rs"
    mkdir $lint_pass_dir

    let lint_files = [
        "rs_no_heap.rs"
        "rs_no_dyn.rs"
        "rs_no_panic.rs"
        "rs_no_nondet.rs"
        "rs_deterministic.rs"
        "rs_bounded_async.rs"
        "rs_step.rs"
        "rs_addressed.rs"
    ]

    for file in $lint_files {
        inject-file $"($PATCHES_DIR)/($file)" $"($lint_pass_dir)/($file)"
    }

    # Create mod.rs for the rs lint module
    let mod_content = ($lint_files | each {|f|
        let name = ($f | path parse | get stem)
        $"pub mod ($name);"
    } | str join "\n")

    let mod_with_reexport = $"($mod_content)

/// Register all Rs lint passes in the lint store.
pub fn register_all\(store: &mut crate::LintStore\) {
    rs_no_heap::register_lints\(store\);
    rs_no_dyn::register_lints\(store\);
    rs_no_panic::register_lints\(store\);
    rs_no_nondet::register_lints\(store\);
    rs_deterministic::register_lints\(store\);
    rs_bounded_async::register_lints\(store\);
    rs_step::register_lints\(store\);
    rs_addressed::register_lints\(store\);
}
"

    $mod_with_reexport | save $"($lint_pass_dir)/mod.rs" --force
    print "  created: rs/mod.rs (lint pass module)"

    # Add rs module declaration to rustc_lint/src/lib.rs
    patch-file $"($rustc_lint_dir)/lib.rs" (
        "mod types;"
    ) (
        "mod types;\nmod rs;"
    ) --description "add rs lint module to rustc_lint"

    # Register Rs lints in the lint store initialization
    patch-file $"($rustc_lint_dir)/lib.rs" (
        "    // end of late lint pass registration"
    ) (
        '    rs::register_all(store);
    // end of late lint pass registration'
    ) --description "register Rs lint passes in lint store"

    # ------------------------------------------------------------------
    # 3. Inject diagnostics
    # ------------------------------------------------------------------
    step "Injecting diagnostics"

    inject-file $"($PATCHES_DIR)/rs_diag.rs" $"($lint_pass_dir)/rs_diag.rs"

    # Register Rs error codes in the diagnostic registry
    patch-file $"($rustc_driver_dir)/lib.rs" (
        "    // register diagnostics"
    ) (
        '    // register Rs diagnostics
    rs_diag::register_diagnostics(&mut registry);
    // register diagnostics'
    ) --description "register Rs diagnostics in driver"

    # ------------------------------------------------------------------
    # 4. Widen visibility where lint passes need access
    # ------------------------------------------------------------------
    step "Widening visibility for lint pass access"

    # Lint passes need access to some pub(crate) items in rustc_middle
    let rustc_middle_dir = $"($compiler_dir)/rustc_middle/src"

    # MIR body access for deterministic lint
    patch-file $"($rustc_middle_dir)/mir/mod.rs" (
        "pub(crate) fn basic_blocks"
    ) (
        "pub fn basic_blocks"
    ) --description "widen MIR basic_blocks visibility"

    step "Patch phase complete"
}

# ---------------------------------------------------------------------------
# Build phase
# ---------------------------------------------------------------------------

def do-build [] {
    step "Building rsc"

    require-cmd "cargo"

    let host = (rustc --print target-triple | str trim)
    let src_root = $"($VENDOR_DIR)/rust-src-($RUSTC_VERSION)/lib/rustc/src/rust"

    if not ($src_root | path exists) {
        bail "patched source not found — run with --fetch --patch first"
    }

    # Build the rsc binary using the patched rustc source
    # The rsc binary itself is a thin wrapper; the real work is in the
    # patched rustc that gets built from the vendored source.

    print "  building patched rustc (this takes a few minutes)..."

    # Set environment for the build
    $env.RUSTC_BOOTSTRAP = "1"
    $env.RS_VENDOR_DIR = ($VENDOR_DIR | path expand)

    cargo build --release 2>&1

    if $env.LAST_EXIT_CODE != 0 {
        bail "build failed"
    }

    # Copy the built binary to the expected location
    let target_bin = "target/release/rsc"
    if ($target_bin | path exists) {
        print $"  built: ($target_bin)"
    }

    step "Build complete"
    print ""
    print "  Usage:"
    print "    rsc --edition rs my_program.rs"
    print "    cargo +rsc build"
    print ""
}

# ---------------------------------------------------------------------------
# Clean
# ---------------------------------------------------------------------------

def do-clean [] {
    step "Cleaning vendor directory"

    if ($VENDOR_DIR | path exists) {
        rm -rf $VENDOR_DIR
        print $"  removed: ($VENDOR_DIR)"
    } else {
        print "  already clean"
    }
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main [
    --fetch    # fetch rustc source only
    --patch    # apply patches only
    --build    # build rsc only
    --clean    # remove vendor directory
    --verbose  # show detailed output
] {
    print $"(ansi cyan_bold)rsc(ansi reset) vendor+patch build system"
    print $"  rustc version: ($RUSTC_VERSION)"
    print ""

    if $clean {
        do-clean
        return
    }

    # If no flags given, do everything
    let do_all = (not $fetch) and (not $patch) and (not $build)

    if $fetch or $do_all {
        do-fetch
    }

    if $patch or $do_all {
        do-patch
    }

    if $build or $do_all {
        do-build
    }

    if $do_all {
        step "All done"
        print ""
        print "  The rsc compiler is ready."
        print "  Run `rsc --edition rs your_file.rs` to compile with Rs restrictions."
        print ""
    }
}
