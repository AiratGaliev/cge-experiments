use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let script = manifest_dir.join("assets/game.js");
    println!("cargo:rerun-if-changed={}", script.display());

    if !script.is_file() {
        panic!(
            "compiled TypeScript game logic is missing at {}; run npm ci && npm run build first",
            script.display()
        );
    }

    let source = fs::read_to_string(&script).expect("read compiled game script");
    if !source.contains("globalThis.game") {
        panic!("assets/game.js does not expose the expected globalThis.game API");
    }
}
