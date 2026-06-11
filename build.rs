//! Build script — Windows only.
//!
//! Embeds `branding/warbell.ico` and version metadata into the compiled `.exe`'s resource
//! section. Effects, all from this one embed (no runtime code):
//!   * Explorer / taskbar / Alt-Tab show the Warbell bell instead of the generic Rust exe icon.
//!   * winit uses the exe's icon resource as the default window icon, so the title-bar +
//!     taskbar entry are branded without any `set_window_icon` call at startup.
//!   * The "Details" tab of the file's Properties shows ProductName / Company / version,
//!     and the installer's Add/Remove-Programs entry inherits them.
//! On non-Windows targets (the Linux release build) this is a no-op.

fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("branding/warbell.ico");
        res.set("ProductName", "Warbell");
        res.set("FileDescription", "Warbell — defend the castle against the night siege");
        res.set("CompanyName", "miskibin");
        res.set("LegalCopyright", "© 2026 miskibin");
        // FileVersion / ProductVersion are auto-derived from CARGO_PKG_VERSION by winresource.
        res.compile().expect("failed to embed Windows .exe resources (icon + version metadata)");
    }
}
