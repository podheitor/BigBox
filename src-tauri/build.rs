fn main() {
    // Track frontend files so cargo rebuilds when they change
    println!("cargo:rerun-if-changed=../frontend/index.html");
    println!("cargo:rerun-if-changed=../frontend/style.css");
    println!("cargo:rerun-if-changed=../frontend/app.js");
    // services.json moved to crates/bigbox-config/data/ — its include_str! in
    // that crate auto-tracks changes, so no rerun line is needed here.
    tauri_build::build()
}
