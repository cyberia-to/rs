//! rsc — Rs compiler driver.
//!
//! Thin wrapper around a patched rustc binary. All Rs-specific behavior
//! lives in the injected lint passes; this binary just forwards arguments.
//!
//! Build: `nu patches/apply.nu && cargo build --release`
//! Usage: `rsc --edition rs my_program.rs`

use std::env;
use std::process::{self, Command};

fn main() {
    let vendor_rustc = env::current_exe()
        .expect("cannot determine rsc binary path")
        .parent()
        .expect("binary has no parent directory")
        .join("rsc-rustc");

    if !vendor_rustc.exists() {
        eprintln!(
            "error: patched rustc not found at {}",
            vendor_rustc.display()
        );
        eprintln!("hint: run `nu patches/apply.nu` first to build the patched compiler");
        process::exit(1);
    }

    let args: Vec<String> = env::args().skip(1).collect();
    let status = Command::new(&vendor_rustc)
        .args(&args)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("error: failed to execute {}: {}", vendor_rustc.display(), e);
            process::exit(1);
        });

    process::exit(status.code().unwrap_or(1));
}
