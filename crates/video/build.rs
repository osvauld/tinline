use std::path::PathBuf;

fn main() {
    // Static libaom from scripts/build_libaom.py: `AOM_DIR_<target>` (dashes as underscores, so
    // one build can cover several Android ABIs) or `AOM_DIR`. That is what release and Android
    // builds use; nothing is needed at runtime. Dev host builds without it use the system libaom
    // (dynamic) unless `AOM_STATIC_REQUIRED=1`, which release packaging sets.
    let target = std::env::var("TARGET").unwrap();
    let per_target = format!("AOM_DIR_{}", target.replace('-', "_"));
    println!("cargo:rerun-if-env-changed={per_target}");
    println!("cargo:rerun-if-env-changed=AOM_DIR");
    println!("cargo:rerun-if-env-changed=AOM_STATIC_REQUIRED");
    println!("cargo:rerun-if-changed=csrc/aom_rt.c");
    let hint = format!("run scripts/build_libaom.py --target {} and set {per_target}", if target.contains("android") { &target } else { "host" });
    let mut cc = cc::Build::new();
    cc.file("csrc/aom_rt.c").warnings(true);
    match std::env::var_os(&per_target).or_else(|| std::env::var_os("AOM_DIR")) {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            let lib = dir.join("lib/libaom.a");
            assert!(lib.exists(), "{} missing: {hint}", lib.display());
            println!("cargo:rerun-if-changed={}", lib.display());
            cc.include(dir.join("include"));
            cc.compile("aom_rt");
            println!("cargo:rustc-link-search=native={}", dir.join("lib").display());
            println!("cargo:rustc-link-lib=static=aom");
        }
        None if target.contains("android") || std::env::var_os("AOM_STATIC_REQUIRED").is_some_and(|v| v == "1") => {
            panic!("{per_target} not set and static libaom required: {hint}")
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
