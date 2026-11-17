//! Structured tool failure shared by selector parsing and host tool execution.
//!
//! Ported from `l/src/tools/tool-errors.ts`.
//!
//! Deviation: the TypeScript `ToolError extends Error` carries
//! `readonly context?: Record<string, unknown>`; the Rust port stores the
//! context as `Option<HashMap<String, serde_json::Value>>` and implements
//! [`std::error::Error`]/`Display` instead of extending a JS `Error` class.
//! `render()` keeps the reference's override point: it returns the message.

use std::collections::HashMap;
use std::fmt;

/// A tool failure with an optional structured context payload.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolError {
    message: String,
    context: Option<HashMap<String, serde_json::Value>>,
}

impl ToolError {
    /// A failure with `message` and an optional `context` payload.
    pub fn new(
        message: impl Into<String>,
        context: Option<HashMap<String, serde_json::Value>>,
    ) -> Self {
        ToolError {
            message: message.into(),
            context,
        }
    }

    /// The failure message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The structured context payload, when one was supplied.
    pub fn context(&self) -> Option<&HashMap<String, serde_json::Value>> {
        self.context.as_ref()
    }

    /// Render the error for LLM consumption. Override for custom formatting.
    pub fn render(&self) -> String {
        self.message.clone()
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_and_context_round_trip() {
        let mut context = HashMap::new();
        context.insert("path".to_string(), json!("/tmp/file"));
        let error = ToolError::new("boom", Some(context));
        assert_eq!(error.message(), "boom");
        assert_eq!(error.context().unwrap()["path"], "/tmp/file");
        assert_eq!(error.render(), "boom");
        assert_eq!(error.to_string(), "boom");
    }

    #[test]
    fn context_is_optional() {
        let error = ToolError::new("plain", None);
        assert!(error.context().is_none());
        assert_eq!(error.render(), "plain");
    }

    #[test]
    fn works_as_std_error() {
        let error: Box<dyn std::error::Error> = Box::new(ToolError::new("wrapped", None));
        assert_eq!(error.to_string(), "wrapped");
    }
}
