fn main() {
    // Tell the linker where to find rustc dylibs at runtime.
    let output = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .expect("failed to run rustc --print sysroot");
    let sysroot = String::from_utf8(output.stdout).unwrap();
    let sysroot = sysroot.trim();
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}/lib", sysroot);
}
