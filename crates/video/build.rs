fn main() {
    // Spike: system libaom via pkg-config. Release builds will compile libaom from source and
    // link it statically (like opus), so nothing is needed at runtime.
    let aom = pkg_config::Config::new().atleast_version("3.8").probe("aom").expect("libaom (pkg-config aom)");
    let mut cc = cc::Build::new();
    cc.file("csrc/aom_rt.c").warnings(true);
    for p in &aom.include_paths {
        cc.include(p);
    }
    cc.compile("aom_rt");
    println!("cargo:rerun-if-changed=csrc/aom_rt.c");
}
