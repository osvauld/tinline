use std::path::PathBuf;

fn main() {
    // `AOM_DIR` = install prefix from scripts/build_libaom.py: link that libaom statically (what
    // release and Android builds do, nothing needed at runtime). Without it, the system libaom.
    println!("cargo:rerun-if-env-changed=AOM_DIR");
    println!("cargo:rerun-if-changed=csrc/aom_rt.c");
    let mut cc = cc::Build::new();
    cc.file("csrc/aom_rt.c").warnings(true);
    match std::env::var_os("AOM_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            cc.include(dir.join("include"));
            cc.compile("aom_rt");
            println!("cargo:rustc-link-search=native={}", dir.join("lib").display());
            println!("cargo:rustc-link-lib=static=aom");
        }
        None => {
            let aom = pkg_config::Config::new().atleast_version("3.8").probe("aom").expect("libaom (pkg-config aom)");
            for p in &aom.include_paths {
                cc.include(p);
            }
            cc.compile("aom_rt");
        }
    }
}
