//! Error types for code88 module.

use thiserror::Error;

/// Errors that can occur during 88code login and token operations.
#[derive(Debug, Error)]
pub enum Code88Error {
    /// No compatible browser (Chrome/Edge) found on the system.
    #[error("未找到 Chrome 或 Edge 浏览器，请安装后重试")]
    NoBrowser,

    /// Failed to launch browser process.
    #[error("启动浏览器失败: {0}")]
    BrowserLaunchFailed(String),

    /// Debug port is already in use.
    #[error("调试端口 {0} 被占用")]
    PortInUse(u16),

    /// Failed to connect to browser CDP endpoint.
    #[error("连接浏览器调试接口失败: {0}")]
    CdpConnectionFailed(String),

    /// WebSocket error during CDP communication.
    #[error("WebSocket 通信错误: {0}")]
    WebSocketError(String),

    /// Failed to get response from CDP.
    #[error("获取浏览器响应失败: {0}")]
    CdpResponseError(String),

    /// Login operation timed out.
    #[error("登录超时，请重试")]
    Timeout,

    /// No token found in the API response.
    #[error("API 响应中未找到 token")]
    NoToken,

    /// API returned an error code.
    #[error("API 返回错误码: {0}")]
    ApiError(i32),

    /// Failed to parse API response.
    #[error("解析 API 响应失败: {0}")]
    ParseError(String),

    /// IO error during file operations.
    #[error("文件操作错误: {0}")]
    IoError(String),

    /// HTTP request failed.
    #[error("HTTP 请求失败: {0}")]
    HttpError(String),
}

impl From<std::io::Error> for Code88Error {
    fn from(e: std::io::Error) -> Self {
        Code88Error::IoError(e.to_string())
    }
}

impl From<reqwest::Error> for Code88Error {
    fn from(e: reqwest::Error) -> Self {
        Code88Error::HttpError(e.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Code88Error {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Code88Error::WebSocketError(e.to_string())
    }
}

impl From<serde_json::Error> for Code88Error {
    fn from(e: serde_json::Error) -> Self {
        Code88Error::ParseError(e.to_string())
    }
}
