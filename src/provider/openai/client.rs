//! Native Rust OpenAI provider — HTTP client with retry + backoff.

use anyhow::{Context, Result};
use std::time::Duration;

/// HTTP client wrapper with retry logic.
pub struct HttpClient {
    client: reqwest::Client,
}

macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG")
            .map(|v| v.to_lowercase().contains("debug"))
            .unwrap_or(false)
        {
            crate::observability::tee_eprintln(&format!("[DEBUG OPENAI-NATIVE] {}", format!($($arg)*)));
        }
    };
}

impl HttpClient {
    /// Create a new HTTP client, reading timeout/pool config from env vars.
    pub fn new() -> Result<Self> {
        Self::new_with_tls(false)
    }

    /// Create a new HTTP client with an explicit certificate-verification
    /// policy. `accept_invalid_certs = true` maps to reqwest's
    /// `danger_accept_invalid_certs` — ANY certificate is accepted for the
    /// provider's endpoint (self-signed, private CA, mismatched hostname).
    /// Intended for self-hosted/LAN endpoints; the default (`false`) keeps
    /// full rustls/webpki validation.
    pub fn new_with_tls(accept_invalid_certs: bool) -> Result<Self> {
        let timeout_secs = std::env::var("LLM_TIMEOUT_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120u64);

        let pool_idle_secs = std::env::var("LLM_POOL_IDLE_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600u64);

        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(pool_idle_secs));

        if accept_invalid_certs {
            builder = builder.danger_accept_invalid_certs(true);
        }

        let client = builder.build().context("Failed to create HTTP client")?;

        Ok(Self { client })
    }

    /// Reference to the inner reqwest client.
    pub fn inner(&self) -> &reqwest::Client {
        &self.client
    }

    /// Max retries from env (default 3).
    fn max_retries() -> u32 {
        std::env::var("LLM_MAX_RETRIES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3)
    }

    /// POST with retry: 429 → Retry-After backoff; 5xx → exponential backoff.
    pub async fn post_with_retry(
        &self,
        url: &str,
        body: String,
        api_key: &str,
        stream: bool,
    ) -> Result<reqwest::Response> {
        let max_retries = Self::max_retries();
        let mut last_error = None;

        for attempt in 0..=max_retries {
            let mut request = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .body(body.clone());

            if stream {
                request = request
                    .header("Accept", "text/event-stream")
                    .timeout(Duration::from_secs(600));
            }

            match request.send().await {
                Ok(resp) => {
                    let status = resp.status();

                    if status.is_success() {
                        return Ok(resp);
                    }

                    if status.as_u16() == 429 {
                        let retry_after = resp
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(60);

                        debug!(
                            "Rate limited (429), waiting {}s (attempt {}/{})",
                            retry_after,
                            attempt,
                            max_retries
                        );
                        tokio::time::sleep(Duration::from_secs(retry_after)).await;
                        last_error = Some(anyhow::anyhow!("Rate limited (429)"));
                        continue;
                    }

                    if status.is_server_error() {
                        let error_body = resp.text().await.unwrap_or_default();
                        debug!(
                            "Server error {}: {}, retrying (attempt {}/{})",
                            status, error_body, attempt, max_retries
                        );
                        tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                        last_error =
                            Some(anyhow::anyhow!("Server error {}: {}", status, error_body));
                        continue;
                    }

                    // Client error (non-429) — no retry
                    let error_body = resp.text().await.unwrap_or_default();
                    return Err(anyhow::anyhow!("API error {}: {}", status, error_body));
                }
                Err(e) => {
                    if e.is_timeout() || e.is_connect() {
                        debug!(
                            "Network error: {}, retrying (attempt {}/{})",
                            e, attempt, max_retries
                        );
                        tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                        last_error = Some(e.into());
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Max retries exceeded")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default client (validation ON) and permissive client (validation OFF)
    /// both build cleanly. No network — construction only. The permissive
    /// path is what a self-signed endpoint needs
    /// (danger_accept_invalid_certs from [llm.providers.*]).
    #[test]
    fn test_new_with_tls_builds_both_policies() {
        let strict = HttpClient::new().expect("default client builds");
        let permissive = HttpClient::new_with_tls(true).expect("permissive client builds");
        let explicit_strict = HttpClient::new_with_tls(false).expect("explicit strict builds");
        // Different policies are distinct clients; all three hold a usable
        // reqwest::Client (inner() borrows — proves construction completed).
        let _ = strict.inner();
        let _ = permissive.inner();
        let _ = explicit_strict.inner();
    }
}
