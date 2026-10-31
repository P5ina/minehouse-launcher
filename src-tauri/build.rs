fn main() {
    println!("cargo:rerun-if-env-changed=MINEHOUSE_MS_CLIENT_ID");
    tauri_build::build()
}
