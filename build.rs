// Gives the Windows executable its icon and file details (shown by Explorer,
// the taskbar and the installer). Nothing to do on other systems.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon/simple-git.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/simple-git.ico")
            .set("ProductName", "Simple Git")
            .set("FileDescription", "Simple Git")
            .set("LegalCopyright", "Simple Git contributors");
        res.compile().expect("Windows resources");
    }
}
