//! Native renderer screenshot smoke test.

#[cfg(target_os = "macos")]
#[test]
fn screenshot_renders_initial_ui() {
    if !cfg!(feature = "screenshot-test") || std::env::var_os("TOYBOX_UI_SCREENSHOT").is_none() {
        return;
    }
    let runner = std::env::var_os("CARGO_BIN_EXE_frqx_gpui_screenshot")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let executable = std::env::current_exe().ok()?;
            let target_dir = executable.parent()?.parent()?;
            let direct = target_dir.join("frqx_gpui_screenshot");
            direct.is_file().then_some(direct)
        })
        .expect("Cargo should build the screenshot runner");
    let status = std::process::Command::new(runner)
        .status()
        .expect("GPUI screenshot runner starts");
    assert!(status.success(), "GPUI screenshot runner failed");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn screenshot_renders_initial_ui() {}
