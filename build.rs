//! The embedded linux-x86_64 agent binary is gitignored (it's a build
//! artifact produced by scripts/build-agent.sh). Create an empty placeholder
//! so `include_bytes!` in remote.rs always resolves; remote::ensure_agent
//! refuses to push an empty blob in dev builds.

fn main() {
    let p = std::path::Path::new("assets/agent-linux-x86_64");
    if !p.exists() {
        std::fs::create_dir_all("assets").expect("mkdir assets");
        std::fs::write(p, []).expect("write agent placeholder");
    }
    println!("cargo:rerun-if-changed=assets/agent-linux-x86_64");
}
