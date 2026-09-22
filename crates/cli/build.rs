use std::path::Path;

/// The page is embedded from ui/dist, which only exists once the UI has been built. A
/// stand-in keeps `cargo build` working without it.
fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    if dist.join("index.html").exists() {
        return;
    }
    std::fs::create_dir_all(&dist).expect("ui/dist should be creatable");
    std::fs::write(
        dist.join("index.html"),
        "<!doctype html><p>The UI wasn't built into this binary. Run ./build and try again.</p>\n",
    )
    .expect("ui/dist/index.html should be writable");
}
