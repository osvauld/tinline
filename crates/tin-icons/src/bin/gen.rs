//! Writes crates/tin-icons/generated/* and the playground copy. `--check` exits non-zero instead if any file is stale.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let check = std::env::args().any(|a| a == "--check");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut stale = Vec::new();
    for (name, text) in tin_icons::emit::outputs() {
        let path = dir.join(name);
        let same = std::fs::read_to_string(&path).is_ok_and(|old| old == text);
        if check {
            if !same {
                stale.push(name);
            }
        } else if !same {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("create output dir");
            std::fs::write(&path, text).expect("write generated file");
            println!("wrote {}", path.display());
        }
    }
    if stale.is_empty() {
        ExitCode::SUCCESS
    } else {
        eprintln!("stale: {} (run `cargo run -p tin-icons --bin tin-icons-gen`)", stale.join(", "));
        ExitCode::FAILURE
    }
}
