// The UI is compiled into release builds from ui/out. The folder has to
// exist for that to compile, even when the UI has not been built.
fn main() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/out");
    let _ = std::fs::create_dir_all(&out);
    println!("cargo:rerun-if-changed=../../ui/out");
}
