fn main() {
    // rav1d's assembly addresses Rust statics PC-relatively. In a shared library those symbols
    // are preemptible by default and the link fails; -Bsymbolic binds them inside libp2pcore.so.
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if os == "android" || os == "linux" {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-Bsymbolic");
    }
}
