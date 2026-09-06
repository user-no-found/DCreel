fn main() {
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("windows-msvc")) {
        // Embed the same Common Controls v6 dependency in every executable,
        // including the lib test harness used by native WebView tests. Tauri's
        // resource is binary-only; omit its manifest to avoid embedding it twice.
        // Icons and version information still come from Tauri's resource.
        let attributes = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        tauri_build::try_build(attributes).expect("failed to build Tauri resources");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    } else {
        tauri_build::build();
    }
}
