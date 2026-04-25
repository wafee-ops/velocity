fn main() {
    println!("cargo:rerun-if-changed=assets/velocity.ico");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/velocity.ico");
        resource
            .compile()
            .expect("failed to embed Windows app icon");
    }
}
