fn main(){
    println!("cargo:rerun-if-changed=scripts/generate_icon.py");
    // The build script itself is compiled for the host. On macOS the
    // Windows resource crate is intentionally not a dependency.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default()=="windows"{
        let icon="dist/branding/kvm.ico";
        if std::path::Path::new(icon).exists(){
            let mut resources=winresource::WindowsResource::new();
            resources.set_icon(icon);
            resources.set("ProductName","kvm");
            resources.set("FileDescription","kvm — Cross-device keyboard and mouse");
            resources.compile().expect("Unable to embed Windows icon");
        }else{
            println!("cargo:warning=No app icon generated; run python scripts/generate_icon.py before building");
        }
    }
}
