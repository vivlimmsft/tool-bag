// Compile and embed the Win32 resource file (the .rc beside this) so the
// produced .exe carries the application icon. Both Explorer (file icon)
// and our own runtime tray icon code (`Icon::from_resource`) read this.
//
// The .ico is checked-in (regenerate with `cargo run --example gen_icon`).
fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_resource::compile("assets/webp-tray.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
    println!("cargo:rerun-if-changed=assets/webp-tray.rc");
    println!("cargo:rerun-if-changed=assets/webp-tray.ico");
}
