//! Typed LLM API status error + terminal-4xx classification (always
//! available — `orchestration` compiles without the `provider` feature, so
//! the classifier cannot live there; fffe9141 F2). Anyhow-free: `anyhow` is
//! an OPTIONAL abk dependency and this module compiles under default
//! features (the publish-verify standalone build).
//!
//! The provider clients raise [`ApiStatusError`] for non-success HTTP
//! responses; the orchestration retry branches classify via
//! [`is_terminal_api_error`] and fail fast on terminal statuses instead of
//! burning identical retries on deterministic errors.

use std::error::Error as StdError;
use std::fmt;

/// A non-success HTTP status from an LLM provider, with the response body.
///
/// Display format is the established wire string
/// (`API error {status}: {body}`) so downstream log greps and error-message
/// matching keep working unchanged.
#[derive(Debug)]
pub struct ApiStatusError {
    /// HTTP status code (e.g. 400, 422).
    pub status: u16,
    /// The provider's response body (error message / code payload).
    pub body: String,
}

impl fmt::Display for ApiStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "API error {}: {}", self.status, self.body)
    }
}

impl StdError for ApiStatusError {}

impl ApiStatusError {
    /// Terminal = a deterministic client error the provider will repeat
    /// identically on retry. 408 (request timeout) and 429 (rate limit)
    /// stay retryable; 5xx and transport errors retry as before.
    pub fn is_terminal(&self) -> bool {
        (400..500).contains(&self.status) && self.status != 408 && self.status != 429
    }
}

/// Walk the error chain (via `source()`) for a typed [`ApiStatusError`] and
/// report whether it carries a terminal 4xx status. Untyped errors
/// (transport, timeouts, plain context wrappers) are never terminal.
///
/// Accepts `&dyn Error` — pass an `anyhow::Error` as `&*err` (anyhow derefs
/// to `dyn Error + Send + Sync`).
pub fn is_terminal_api_error(err: &(dyn StdError + 'static)) -> bool {
    let mut current = Some(err);
    while let Some(e) = current {
        if let Some(api_err) = e.downcast_ref::<ApiStatusError>() {
            return api_err.is_terminal();
        }
        current = e.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_classification() {
        for status in [400u16, 401, 403, 404, 422] {
            assert!(
                ApiStatusError {
                    status,
                    body: "x".into()
                }
                .is_terminal(),
                "{status} must be terminal"
            );
        }
        for status in [408u16, 429] {
            assert!(
                !ApiStatusError {
                    status,
                    body: "x".into()
                }
                .is_terminal(),
                "{status} must stay retryable"
            );
        }
        for status in [500u16, 502, 503] {
            assert!(
                !ApiStatusError {
                    status,
                    body: "x".into()
                }
                .is_terminal(),
                "{status} must stay retryable"
            );
        }
    }

    #[test]
    fn display_keeps_the_wire_format() {
        let e = ApiStatusError {
            status: 400,
            body: "{\"code\":\"1211\"}".into(),
        };
        assert_eq!(e.to_string(), "API error 400: {\"code\":\"1211\"}");
    }

    /// A minimal context wrapper (mirrors anyhow's `.context()` layering)
    /// proving the chain walk reaches the typed error through `source()`.
    struct Wrapped(Box<dyn StdError + 'static>);

    impl fmt::Display for Wrapped {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "context wrapper")
        }
    }

    impl StdError for Wrapped {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            Some(self.0.as_ref())
        }
    }

    #[test]
    fn chain_walk_finds_typed_error_through_context() {
        let wrapped = Wrapped(Box::new(ApiStatusError {
            status: 400,
            body: "bad".into(),
        }));
        let e: &(dyn StdError + 'static) = &wrapped;
        assert!(is_terminal_api_error(e));
    }

    #[test]
    fn untyped_errors_are_never_terminal() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "connection refused");
        assert!(!is_terminal_api_error(&io_err));
        let retryable = ApiStatusError {
            status: 429,
            body: "rate limited".into(),
        };
        assert!(!is_terminal_api_error(&retryable));
    }
}
