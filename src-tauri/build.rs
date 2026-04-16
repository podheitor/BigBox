fn main() {
    // Track frontend files so cargo rebuilds when they change
    println!("cargo:rerun-if-changed=../frontend/index.html");
    println!("cargo:rerun-if-changed=../frontend/style.css");
    println!("cargo:rerun-if-changed=../frontend/app.js");
    println!("cargo:rerun-if-changed=../data/services.json");
    tauri_build::build()
}
