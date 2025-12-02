//! Simplified Chrome DevTools Protocol (CDP) client.
//!
//! This module implements a minimal CDP client for network monitoring.
//! It only supports the features needed for capturing login responses.

use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::debug;
use tracing::trace;

use crate::Code88Error;

/// CDP session for communicating with browser.
pub struct CdpSession {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    msg_id: AtomicU32,
}

impl CdpSession {
    /// Connect to browser's CDP endpoint.
    ///
    /// # Arguments
    /// * `debug_url` - The browser's debug URL (e.g., "http://localhost:9222")
    pub async fn connect(debug_url: &str) -> Result<Self, Code88Error> {
        // 1. Get list of debuggable pages
        let json_url = format!("{debug_url}/json");
        debug!("Fetching CDP targets from: {}", json_url);

        let targets: Vec<Value> = reqwest::get(&json_url)
            .await
            .map_err(|e| Code88Error::CdpConnectionFailed(format!("HTTP request failed: {e}")))?
            .json()
            .await
            .map_err(|e| Code88Error::CdpConnectionFailed(format!("JSON parse failed: {e}")))?;

        // 2. Find the first page target
        let ws_url = targets
            .iter()
            .find(|t| t["type"] == "page")
            .and_then(|t| t["webSocketDebuggerUrl"].as_str())
            .ok_or_else(|| {
                Code88Error::CdpConnectionFailed("No debuggable page found".to_string())
            })?;

        debug!("Connecting to CDP WebSocket: {}", ws_url);

        // 3. Connect via WebSocket
        let (ws, _response) = connect_async(ws_url).await.map_err(|e| {
            Code88Error::CdpConnectionFailed(format!("WebSocket connection failed: {e}"))
        })?;

        debug!("CDP WebSocket connected");

        Ok(Self {
            ws,
            msg_id: AtomicU32::new(0),
        })
    }

    /// Send a CDP command and wait for response.
    async fn send_command(&mut self, method: &str, params: Value) -> Result<Value, Code88Error> {
        let id = self.msg_id.fetch_add(1, Ordering::SeqCst) + 1;

        let msg = json!({
            "id": id,
            "method": method,
            "params": params
        });

        trace!("Sending CDP command: {} (id={})", method, id);

        self.ws
            .send(Message::Text(msg.to_string()))
            .await
            .map_err(|e| Code88Error::WebSocketError(e.to_string()))?;

        // Wait for the response with matching id
        loop {
            let msg = self
                .ws
                .next()
                .await
                .ok_or_else(|| Code88Error::CdpResponseError("Connection closed".to_string()))?
                .map_err(|e| Code88Error::WebSocketError(e.to_string()))?;

            if let Message::Text(text) = msg {
                let data: Value = serde_json::from_str(&text)?;

                // Check if this is a response to our command
                if data.get("id").and_then(Value::as_u64) == Some(id as u64) {
                    if let Some(error) = data.get("error") {
                        return Err(Code88Error::CdpResponseError(error.to_string()));
                    }
                    return Ok(data);
                }

                // If it's an event, log it and continue waiting
                if data.get("method").is_some() {
                    trace!(
                        "Received CDP event while waiting for response: {:?}",
                        data.get("method")
                    );
                }
            }
        }
    }

    /// Enable network monitoring.
    pub async fn enable_network(&mut self) -> Result<(), Code88Error> {
        debug!("Enabling CDP Network domain");
        self.send_command("Network.enable", json!({})).await?;
        Ok(())
    }

    /// Wait for a network response matching the given URL pattern.
    ///
    /// Returns the response body when a matching response is received.
    pub async fn wait_for_response(&mut self, url_pattern: &str) -> Result<String, Code88Error> {
        debug!("Waiting for response matching: {}", url_pattern);

        loop {
            let msg = self
                .ws
                .next()
                .await
                .ok_or_else(|| Code88Error::CdpResponseError("Connection closed".to_string()))?
                .map_err(|e| Code88Error::WebSocketError(e.to_string()))?;

            if let Message::Text(text) = msg {
                let data: Value = serde_json::from_str(&text)?;

                // Check for Network.responseReceived event
                if data.get("method") == Some(&json!("Network.responseReceived"))
                    && let Some(params) = data.get("params")
                {
                    let response_url = params
                        .get("response")
                        .and_then(|r| r.get("url"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("");

                    trace!("Network response: {}", response_url);

                    if response_url.contains(url_pattern) {
                        debug!("Found matching response: {}", response_url);

                        // Get the request ID to fetch the body
                        let request_id = params
                            .get("requestId")
                            .ok_or_else(|| {
                                Code88Error::CdpResponseError("Missing requestId".to_string())
                            })?
                            .clone();

                        // Small delay to ensure response body is ready
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                        // Fetch the response body
                        return self.get_response_body(&request_id).await;
                    }
                }

                // Also check for Network.loadingFinished which might help with timing
                if data.get("method") == Some(&json!("Network.loadingFinished")) {
                    trace!("Network loading finished event received");
                }
            }
        }
    }

    /// Get the response body for a given request ID.
    async fn get_response_body(&mut self, request_id: &Value) -> Result<String, Code88Error> {
        debug!("Fetching response body for request: {:?}", request_id);

        let response = self
            .send_command(
                "Network.getResponseBody",
                json!({
                    "requestId": request_id
                }),
            )
            .await?;

        let result = response
            .get("result")
            .ok_or_else(|| Code88Error::CdpResponseError("No result in response".to_string()))?;

        let body = result
            .get("body")
            .and_then(|b| b.as_str())
            .ok_or_else(|| Code88Error::CdpResponseError("No body in response".to_string()))?;

        // Check if body is base64 encoded
        let is_base64 = result
            .get("base64Encoded")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if is_base64 {
            debug!("Response body is base64 encoded, decoding...");
            use base64::Engine;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(body)
                .map_err(|e| Code88Error::ParseError(format!("Base64 decode failed: {e}")))?;
            String::from_utf8(decoded)
                .map_err(|e| Code88Error::ParseError(format!("UTF-8 decode failed: {e}")))
        } else {
            Ok(body.to_string())
        }
    }

    /// Navigate to a URL.
    #[allow(dead_code)]
    pub async fn navigate(&mut self, url: &str) -> Result<(), Code88Error> {
        debug!("Navigating to: {}", url);
        self.send_command("Page.navigate", json!({ "url": url }))
            .await?;
        Ok(())
    }

    /// Reload the current page.
    pub async fn reload(&mut self) -> Result<(), Code88Error> {
        debug!("Reloading page");
        // Enable Page domain first if not already enabled
        let _ = self.send_command("Page.enable", json!({})).await;
        self.send_command("Page.reload", json!({ "ignoreCache": false }))
            .await?;
        Ok(())
    }

    /// Close the CDP session.
    pub async fn close(mut self) -> Result<(), Code88Error> {
        debug!("Closing CDP session");
        self.ws
            .close(None)
            .await
            .map_err(|e| Code88Error::WebSocketError(e.to_string()))?;
        Ok(())
    }
}
