fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=windows_resources.rc");
    embed_resource::compile("windows_resources.rc", embed_resource::NONE).manifest_optional().unwrap();
}
