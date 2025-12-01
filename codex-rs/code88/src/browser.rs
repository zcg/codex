//! Browser detection and launch with remote debugging.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use tracing::{debug, info};

use crate::Code88Error;

/// Default CDP debug port.
const DEFAULT_DEBUG_PORT: u16 = 9222;

/// Alternative ports to try if default is in use.
const ALTERNATIVE_PORTS: &[u16] = &[9223, 9224, 9225, 9226];

/// A running browser instance with remote debugging enabled.
pub struct BrowserInstance {
    process: Option<Child>,
    pub debug_port: u16,
}

impl BrowserInstance {
    /// Get the debug URL for CDP connection.
    pub fn debug_url(&self) -> String {
        format!("http://localhost:{}", self.debug_port)
    }

    /// Kill the browser process.
    pub fn kill(&mut self) {
        if let Some(ref mut process) = self.process {
            let _ = process.kill();
            debug!("Browser process killed");
        }
        self.process = None;
    }
}

impl Drop for BrowserInstance {
    fn drop(&mut self) {
        // Don't auto-kill on drop - let user decide
        // self.kill();
    }
}

/// Detect a Chromium-based browser on the system.
///
/// Searches for Chrome, Edge, or Chromium in common installation paths.
/// Returns the path to the browser executable if found.
pub fn detect_browser() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        detect_browser_windows()
    }

    #[cfg(target_os = "macos")]
    {
        detect_browser_macos()
    }

    #[cfg(target_os = "linux")]
    {
        detect_browser_linux()
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn detect_browser_windows() -> Option<PathBuf> {
    let candidates = [
        // Edge (preferred on Windows)
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        // Chrome
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        // Chrome in user profile
        &format!(
            r"{}\AppData\Local\Google\Chrome\Application\chrome.exe",
            std::env::var("USERPROFILE").unwrap_or_default()
        ),
    ];

    for path_str in &candidates {
        let path = PathBuf::from(path_str);
        if path.exists() {
            info!("Found browser: {:?}", path);
            return Some(path);
        }
    }

    // Try using `where` command as fallback
    if let Ok(output) = Command::new("where").arg("msedge").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = path_str.lines().next() {
                let path = PathBuf::from(line.trim());
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }

    if let Ok(output) = Command::new("where").arg("chrome").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = path_str.lines().next() {
                let path = PathBuf::from(line.trim());
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn detect_browser_macos() -> Option<PathBuf> {
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        // User-level installations
        &format!(
            "{}/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            std::env::var("HOME").unwrap_or_default()
        ),
    ];

    for path_str in &candidates {
        let path = PathBuf::from(path_str);
        if path.exists() {
            info!("Found browser: {:?}", path);
            return Some(path);
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn detect_browser_linux() -> Option<PathBuf> {
    // Try `which` for common browser names
    let browser_names = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
        "microsoft-edge-stable",
    ];

    for name in &browser_names {
        if let Ok(path) = which::which(name) {
            info!("Found browser via which: {:?}", path);
            return Some(path);
        }
    }

    // Fallback to common paths
    let candidates = [
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/microsoft-edge",
        "/snap/bin/chromium",
    ];

    for path_str in &candidates {
        let path = PathBuf::from(path_str);
        if path.exists() {
            info!("Found browser: {:?}", path);
            return Some(path);
        }
    }

    None
}

/// Check if a port is available by attempting to bind to it.
fn is_port_available(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Find an available debug port.
fn find_available_port() -> Option<u16> {
    if is_port_available(DEFAULT_DEBUG_PORT) {
        return Some(DEFAULT_DEBUG_PORT);
    }

    for &port in ALTERNATIVE_PORTS {
        if is_port_available(port) {
            return Some(port);
        }
    }

    None
}

/// Launch a browser with remote debugging enabled.
///
/// # Arguments
/// * `browser_path` - Path to the browser executable
/// * `url` - Initial URL to navigate to
///
/// # Returns
/// A `BrowserInstance` containing the process handle and debug port.
pub fn launch_with_debug(browser_path: &Path, url: &str) -> Result<BrowserInstance, Code88Error> {
    let port = find_available_port().ok_or(Code88Error::PortInUse(DEFAULT_DEBUG_PORT))?;

    info!("Launching browser with debug port {}", port);

    // Build command with appropriate flags
    let mut cmd = Command::new(browser_path);

    // Common flags for all platforms
    cmd.args([
        &format!("--remote-debugging-port={port}"),
        "--no-first-run",
        "--no-default-browser-check",
        // Create a separate user data directory to avoid conflicts
        &format!(
            "--user-data-dir={}",
            temp_user_data_dir().to_string_lossy()
        ),
        url,
    ]);

    // Platform-specific flags
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW flag to prevent console window
        cmd.creation_flags(0x08000000);
    }

    let process = cmd
        .spawn()
        .map_err(|e| Code88Error::BrowserLaunchFailed(e.to_string()))?;

    Ok(BrowserInstance {
        process: Some(process),
        debug_port: port,
    })
}

/// Get a temporary directory for browser user data.
fn temp_user_data_dir() -> PathBuf {
    let temp_dir = std::env::temp_dir();
    temp_dir.join("codex-code88-browser-profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_browser() {
        // This test will pass or skip depending on the system
        let browser = detect_browser();
        if let Some(path) = browser {
            assert!(path.exists());
        }
    }

    #[test]
    fn test_port_availability() {
        // Default port might be in use, but function should work
        let available = is_port_available(DEFAULT_DEBUG_PORT);
        // Just verify it runs without panic
        let _ = available;
    }
}
