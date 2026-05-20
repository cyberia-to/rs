fn main() {
    // Make the rustc_private crates findable.
    let sysroot = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .expect("rustc --print sysroot failed")
        .stdout;
    let sysroot = String::from_utf8(sysroot).unwrap();
    let sysroot = sysroot.trim();

    // Add the sysroot lib dir so that `extern crate rustc_*` resolve.
    let target = std::env::var("TARGET").unwrap_or_else(|_| "aarch64-apple-darwin".into());
    let lib_path = format!("{sysroot}/lib/rustlib/{target}/lib");
    println!("cargo:rustc-link-search=native={lib_path}");
    println!("cargo:rerun-if-env-changed=SYSROOT");
}
