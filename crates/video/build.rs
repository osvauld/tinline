use std::path::PathBuf;

fn main() {
    // Static libaom from scripts/build_libaom.py: `AOM_DIR_<target>` (dashes as underscores, so
    // one build can cover several Android ABIs) or `AOM_DIR`. That is what release and Android
    // builds use; nothing is needed at runtime. Host builds without it use the system libaom.
    let target = std::env::var("TARGET").unwrap();
    let per_target = format!("AOM_DIR_{}", target.replace('-', "_"));
    println!("cargo:rerun-if-env-changed={per_target}");
    println!("cargo:rerun-if-env-changed=AOM_DIR");
    println!("cargo:rerun-if-changed=csrc/aom_rt.c");
    let mut cc = cc::Build::new();
    cc.file("csrc/aom_rt.c").warnings(true);
    match std::env::var_os(&per_target).or_else(|| std::env::var_os("AOM_DIR")) {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            cc.include(dir.join("include"));
            cc.compile("aom_rt");
            println!("cargo:rustc-link-search=native={}", dir.join("lib").display());
            println!("cargo:rustc-link-lib=static=aom");
        }
        None if target.contains("android") => {
            panic!("{per_target} not set: run scripts/build_libaom.py --target {target}")
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
