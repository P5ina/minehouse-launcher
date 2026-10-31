fn main() {
    // auth::client_id() reads it with option_env!.
    println!("cargo:rerun-if-env-changed=MINEHOUSE_MS_CLIENT_ID");
}
