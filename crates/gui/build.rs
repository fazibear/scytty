fn main() {
    println!("cargo:rerun-if-changed=../../assets/scytty.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // GPUI loads the executable's icon resource with ID 1.
        winresource::WindowsResource::new()
            .set_icon("../../assets/scytty.ico")
            .compile()
            .expect("compile Windows application icon");
    }
}
