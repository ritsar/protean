use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;

use crate::config::TARGET_WINDOW_CLASS;

#[derive(Deserialize)]
struct HyprlandWindow {
    class: String,
}

/// Check if the target window is currently active (Hyprland specific)
/// Returns Ok(true) if target window is active, Ok(false) otherwise
/// Returns Err if unable to query Hyprland
pub fn check_active_window() -> Result<bool> {
    let output = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .context("Failed to execute hyprctl - is Hyprland running?")?;

    if !output.status.success() {
        return Ok(false);
    }

    let json_str = String::from_utf8(output.stdout)
        .context("hyprctl returned invalid UTF-8")?;
    
    let window: HyprlandWindow = serde_json::from_str(&json_str)
        .context("Failed to parse hyprctl JSON output")?;
    
    Ok(window.class == TARGET_WINDOW_CLASS)
}
