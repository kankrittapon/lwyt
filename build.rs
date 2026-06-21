fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("app.ico");
        resource.set("ProductName", "LYTBokkChoYx");
        resource.set("FileDescription", "LYTBokkChoYx URL Media Player");
        resource
            .compile()
            .expect("failed to compile Windows resources");
    }
}
