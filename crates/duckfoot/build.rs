//! On Windows, gives `duckfoot.exe` its icon and its version (Explorer's Properties, the
//! taskbar), from `brand/duckfoot.ico` and the crate's version. Other targets: nothing.
//!
//! Needs a resource compiler: `rc.exe` from the Windows SDK, as CI's windows runner has,
//! or `llvm-rc`/`windres` when cross-compiling (see docs/BUILDING.md).

fn main() {
    println!("cargo:rerun-if-changed=../../brand/duckfoot.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../brand/duckfoot.ico");
    res.set("ProductName", "Duckfoot");
    res.set("FileDescription", "Duckfoot photo editor");
    if let Err(e) = res.compile() {
        // A missing resource compiler costs the icon, not the build.
        println!("cargo:warning=no Windows resources (icon, version): {e}");
    }
}
