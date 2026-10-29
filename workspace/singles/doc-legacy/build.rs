use std::path::Path;

fn main() {
    let ucrt = Path::new("C:/msys64/ucrt64");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../.3rd");
    let cdr = root.join("libcdr-install");
    let wpg = root.join("libwpg-install");
    let zmf = root.join("libzmf-install");
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows-gnu") {
        panic!("doc-legacy requires the x86_64-pc-windows-gnu target (MinGW/UCRT libraries)");
    }

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .compiler(ucrt.join("bin/g++.exe"))
        .file("cpp/legacy_shim.cpp")
        .include(ucrt.join("include/librevenge-0.0"))
        .include(cdr.join("include/libcdr-0.1"))
        .include(wpg.join("include/libwpg-0.3"))
        .include(zmf.join("include/libzmf-0.0"))
        .flag("-std=c++17")
        .flag("-O2");
    build.compile("rupix_doc_legacy_shim");

    println!(
        "cargo:rustc-link-search=native={}",
        cdr.join("lib").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        wpg.join("lib").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        zmf.join("lib").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        ucrt.join("lib").display()
    );
    for lib in [
        "cdr-0.1",
        "wpg-0.3",
        "zmf-0.0",
        "wpd-0.10",
        "revenge-0.0",
        "revenge-generators-0.0",
        "revenge-stream-0.0",
        "icuuc",
        "icuin",
        "icuio",
        "icudt",
        "lcms2",
        "png",
        "z",
        "stdc++",
        "winpthread",
    ] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    println!("cargo:rustc-link-lib=advapi32");
    println!("cargo:rustc-link-arg=-static-libgcc");
    println!("cargo:rustc-link-arg=-static-libstdc++");
    println!("cargo:rerun-if-changed=cpp/legacy_shim.cpp");
}
