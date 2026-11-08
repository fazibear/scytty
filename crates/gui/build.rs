use sha2::{Digest, Sha256};

fn main() {
    println!("cargo:rerun-if-env-changed=SCYTTY_SERVER_BUNDLE");
    let mut generated = String::from("const SERVERS: &[Server] = &[\n");
    if let Some(directory) = std::env::var_os("SCYTTY_SERVER_BUNDLE") {
        let directory = std::path::PathBuf::from(directory)
            .canonicalize()
            .expect("SCYTTY_SERVER_BUNDLE directory");
        println!("cargo:rerun-if-changed={}", directory.display());
        let mut count = 0;
        for platform in ["linux-x86_64", "linux-arm64", "macos-arm64", "macos-x86_64"] {
            let path = directory.join(format!("scytty-server-{platform}"));
            if !path.exists() {
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            let bytes = std::fs::read(&path).expect("read bundled server");
            assert!(
                !bytes.is_empty(),
                "empty bundled server: {}",
                path.display()
            );
            let digest = format!("{:x}", Sha256::digest(&bytes));
            generated.push_str(&format!("Server {{ platform: {platform:?}, digest: {digest:?}, bytes: include_bytes!({path:?}) }},\n"));
            count += 1;
        }
        assert!(
            count > 0,
            "SCYTTY_SERVER_BUNDLE contains no supported server binaries"
        );
    }
    generated.push_str("];\n");
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("servers.rs"),
        generated,
    )
    .expect("generate embedded server table");
    println!("cargo:rerun-if-changed=../../assets/scytty.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // GPUI loads the executable's icon resource with ID 1.
        winresource::WindowsResource::new()
            .set_icon("../../assets/scytty.ico")
            .compile()
            .expect("compile Windows application icon");
    }
}
