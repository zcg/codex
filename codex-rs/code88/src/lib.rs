//! 88code.org login and token management module.
//!
//! This module provides functionality to:
//! - Detect and launch Chrome/Edge browser with remote debugging
//! - Connect to browser via Chrome DevTools Protocol (CDP)
//! - Monitor network requests to capture login token
//! - Store and retrieve token from local file

mod browser;
mod cdp;
mod error;
mod token;

pub use error::Code88Error;
pub use token::{delete_token, load_token, save_token, token_path};

use std::path::Path;
use std::time::Duration;

use tracing::{info, warn};

const LOGIN_URL: &str = "https://www.88code.org/";
const TOKEN_API_PATTERN: &str = "/admin-api/login/getLoginInfo";
const DEFAULT_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Result type for code88 operations.
pub type Result<T> = std::result::Result<T, Code88Error>;

/// Ensure a valid 88code token exists.
///
/// If a token already exists in the config directory, it is returned directly.
/// Otherwise, this function will:
/// 1. Launch browser with remote debugging enabled
/// 2. Navigate to 88code.org login page
/// 3. Monitor network requests for the login API response
/// 4. Extract and save the token
///
/// Returns the token string on success.
pub async fn ensure_token(codex_home: &Path) -> Result<String> {
    // Check for existing token first
    if let Some(existing_token) = load_token(codex_home) {
        info!("Found existing 88code token");
        return Ok(existing_token);
    }

    info!("No 88code token found, starting browser login flow");
    run_browser_login(codex_home, DEFAULT_TIMEOUT_SECS).await
}

/// Run the browser login flow to obtain a token.
///
/// This is the main entry point for the login process when no token exists.
pub async fn run_browser_login(codex_home: &Path, timeout_secs: u64) -> Result<String> {
    eprintln!("\n88code: 首次使用，需要登录获取 token...");

    // 1. Detect browser
    let browser_path = browser::detect_browser().ok_or(Code88Error::NoBrowser)?;
    info!("Detected browser: {:?}", browser_path);

    // 2. Launch browser with remote debugging
    let mut instance = browser::launch_with_debug(&browser_path, LOGIN_URL)?;
    info!("Browser launched with debug port: {}", instance.debug_port);

    // 3. Wait for browser to start
    eprintln!("88code: 正在启动浏览器...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 4. Connect to CDP and monitor network
    let result = async {
        let mut cdp = cdp::CdpSession::connect(&instance.debug_url()).await?;
        cdp.enable_network().await?;

        eprintln!("88code: 请在浏览器中完成登录，登录成功后将自动获取 token...\n");

        // 5. Wait for login response
        let body = cdp.wait_for_response(TOKEN_API_PATTERN).await?;

        // 6. Parse token from response
        let token = parse_token_from_response(&body)?;

        // 7. Close CDP session
        let _ = cdp.close().await;

        Ok::<String, Code88Error>(token)
    };

    // Apply timeout
    let token = match tokio::time::timeout(Duration::from_secs(timeout_secs), result).await {
        Ok(Ok(token)) => token,
        Ok(Err(e)) => {
            instance.kill();
            return Err(e);
        }
        Err(_) => {
            instance.kill();
            return Err(Code88Error::Timeout);
        }
    };

    // 8. Save token
    save_token(codex_home, &token)?;
    eprintln!("\n88code: 登录成功！Token 已保存。\n");

    // Clean up browser (optional - user might want to keep it)
    instance.kill();

    Ok(token)
}

/// Parse token from the API response body.
fn parse_token_from_response(body: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct Response {
        code: i32,
        ok: bool,
        data: Option<Data>,
    }

    #[derive(serde::Deserialize)]
    struct Data {
        token: String,
    }

    let resp: Response =
        serde_json::from_str(body).map_err(|e| Code88Error::ParseError(e.to_string()))?;

    if !resp.ok || resp.code != 0 {
        return Err(Code88Error::ApiError(resp.code));
    }

    resp.data
        .map(|d| d.token)
        .ok_or_else(|| Code88Error::NoToken)
}

/// Prompt user for manual token input as fallback.
pub fn prompt_manual_token_input() -> Result<String> {
    eprintln!("\n88code: 无法自动获取 token，请手动输入：");
    eprintln!("  1. 在浏览器中访问 https://www.88code.org/ 并登录");
    eprintln!("  2. 打开开发者工具 (F12) -> Network 标签");
    eprintln!("  3. 刷新页面，找到 getLoginInfo 请求");
    eprintln!("  4. 在响应中找到 token 字段的值并复制");
    eprintln!("\n请输入 token: ");

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| Code88Error::IoError(e.to_string()))?;

    let token = input.trim().to_string();
    if token.is_empty() {
        return Err(Code88Error::NoToken);
    }

    Ok(token)
}

/// Ensure token with fallback to manual input.
pub async fn ensure_token_with_fallback(codex_home: &Path) -> Result<String> {
    match ensure_token(codex_home).await {
        Ok(token) => Ok(token),
        Err(e) => {
            warn!("Auto login failed: {}, falling back to manual input", e);
            let token = prompt_manual_token_input()?;
            save_token(codex_home, &token)?;
            Ok(token)
        }
    }
}
