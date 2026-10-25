fn main() {
    println!("cargo:rerun-if-env-changed=ZIRTOLA_API_BASE");
    tauri_build::build()
}
