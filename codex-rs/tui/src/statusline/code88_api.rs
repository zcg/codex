//! 88code API client for fetching usage information.
//!
//! This module provides async functions to query the 88code usage API
//! and returns structured data for display in the status line.

use serde::Deserialize;
use std::time::Duration;

/// API endpoint for 88code usage information.
const API_URL: &str = "https://www.88code.org/api/usage";

/// Request timeout in seconds.
const TIMEOUT_SECS: u64 = 5;

/// Response wrapper from the 88code API.
#[derive(Debug, Deserialize)]
pub(crate) struct Code88Response {
    pub code: i32,
    pub ok: bool,
    pub data: Option<Code88Data>,
}

/// Usage data returned by the 88code API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Code88Data {
    /// Subscription tier name (e.g., "PRO", "FREE").
    pub subscription_name: Option<String>,
    /// Total credit limit for the subscription.
    pub credit_limit: Option<f64>,
    /// Current remaining credits.
    pub current_credits: Option<f64>,
}

/// Error types for 88code API requests.
#[derive(Debug)]
pub(crate) enum Code88Error {
    /// Network or connection error.
    Network(String),
    /// HTTP status code error.
    HttpStatus(u16),
    /// JSON parsing error.
    Parse(String),
    /// API returned no data.
    NoData,
    /// API returned an error code.
    ApiError(i32),
}

impl std::fmt::Display for Code88Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Code88Error::Network(msg) => write!(f, "Network error: {msg}"),
            Code88Error::HttpStatus(code) => write!(f, "HTTP status error: {code}"),
            Code88Error::Parse(msg) => write!(f, "Parse error: {msg}"),
            Code88Error::NoData => write!(f, "No data returned"),
            Code88Error::ApiError(code) => write!(f, "API error code: {code}"),
        }
    }
}

impl std::error::Error for Code88Error {}

/// Fetches 88code usage information from the API.
///
/// # Arguments
/// * `api_key` - The API key for authentication (Bearer token).
///
/// # Returns
/// * `Ok(Code88Data)` - Usage data on success.
/// * `Err(Code88Error)` - Error information on failure.
pub(crate) async fn fetch_88code_usage(api_key: &str) -> Result<Code88Data, Code88Error> {
    // Build a client that mimics curl behavior
    let client = reqwest::Client::builder()
        .user_agent("curl/8.0")
        .build()
        .map_err(|e| Code88Error::Network(e.to_string()))?;

    let response = client
        .post(API_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "*/*")
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| Code88Error::Network(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(Code88Error::HttpStatus(status.as_u16()));
    }

    let body: Code88Response = response
        .json()
        .await
        .map_err(|e| Code88Error::Parse(e.to_string()))?;

    if body.ok && body.code == 0 {
        body.data.ok_or(Code88Error::NoData)
    } else {
        Err(Code88Error::ApiError(body.code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code88_data_deserialize() {
        // Use actual API response format
        let json = r#"{
            "code": 0,
            "level": null,
            "msg": "操作成功",
            "ok": true,
            "data": {
                "id": 27995,
                "subscriptionName": "FREE",
                "creditLimit": 20.0000000000,
                "currentCredits": 6.3234955000,
                "totalCost": 117.022309
            }
        }"#;

        let response: Code88Response = serde_json::from_str(json).unwrap();
        assert!(response.ok);
        assert_eq!(response.code, 0);

        let data = response.data.unwrap();
        assert_eq!(data.subscription_name.as_deref(), Some("FREE"));
        assert_eq!(data.credit_limit, Some(20.0));
        assert_eq!(data.current_credits, Some(6.3234955000));
    }

    #[test]
    fn test_code88_error_response() {
        let json = r#"{
            "code": -1,
            "ok": false,
            "data": null
        }"#;

        let response: Code88Response = serde_json::from_str(json).unwrap();
        assert!(!response.ok);
        assert_eq!(response.code, -1);
        assert!(response.data.is_none());
    }

    #[test]
    fn test_code88_error_display() {
        assert_eq!(
            Code88Error::Network("timeout".to_string()).to_string(),
            "Network error: timeout"
        );
        assert_eq!(
            Code88Error::HttpStatus(401).to_string(),
            "HTTP status error: 401"
        );
        assert_eq!(
            Code88Error::Parse("invalid json".to_string()).to_string(),
            "Parse error: invalid json"
        );
        assert_eq!(Code88Error::NoData.to_string(), "No data returned");
        assert_eq!(Code88Error::ApiError(-1).to_string(), "API error code: -1");
    }
}
