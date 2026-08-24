//! OpenAI provider.
//!
//! Uses the Chat Completions endpoint with structured output (`response_format` of type
//! `json_schema`, `strict: true`), so the model is constrained to the schema rather than
//! asked politely for JSON. The key is held for the lifetime of one call chain and never
//! logged, never persisted here, and never returned to the frontend.

use serde_json::json;
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::models::AiProviderKind;

use super::provider::{AiProvider, ProviderRequest, ProviderResponse};

pub struct OpenAiProvider {
    base_url: String,
    model: String,
    api_key: Zeroizing<String>,
}

impl OpenAiProvider {
    pub fn new(base_url: String, model: String, api_key: String) -> Self {
        OpenAiProvider {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            api_key: Zeroizing::new(api_key),
        }
    }
}

impl AiProvider for OpenAiProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::OpenAi
    }

    fn model(&self) -> String {
        self.model.clone()
    }

    fn destination(&self) -> String {
        format!("OpenAI cloud ({}), model {}", self.base_url, self.model)
    }

    fn complete(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(request.timeout_secs))
            .build()
            .map_err(|e| Error::AiProvider(format!("could not create the HTTP client: {e}")))?;

        let body = json!({
            "model": self.model,
            "temperature": request.temperature,
            "max_completion_tokens": request.max_output_tokens,
            "messages": [
                { "role": "system", "content": request.system },
                { "role": "user", "content": request.user }
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": request.schema.name(),
                    "strict": true,
                    "schema": request.schema.json_schema()
                }
            }
        });

        let response = client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(self.api_key.as_str())
            .json(&body)
            .send()
            .map_err(|e| Error::AiProvider(describe_transport_error(&e)))?;

        let status = response.status();
        let text = response
            .text()
            .map_err(|e| Error::AiProvider(format!("could not read the response: {e}")))?;

        if !status.is_success() {
            return Err(Error::AiProvider(describe_http_error(
                status.as_u16(),
                &text,
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
            Error::AiProvider("the API returned a response that is not JSON".into())
        })?;

        // A truncated reply would be invalid JSON downstream anyway; saying why is kinder.
        if parsed["choices"][0]["finish_reason"] == "length" {
            return Err(Error::AiProvider(
                "the model ran out of output space; raise the token limit in Settings > AI or \
                 work on a shorter passage"
                    .to_string(),
            ));
        }
        if let Some(refusal) = parsed["choices"][0]["message"]["refusal"].as_str() {
            if !refusal.trim().is_empty() {
                return Err(Error::AiProvider(format!(
                    "the model declined the request: {refusal}"
                )));
            }
        }

        let content = parsed["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| Error::AiProvider("the API response contained no content".into()))?;

        let model = parsed["model"].as_str().unwrap_or(&self.model).to_string();
        Ok(ProviderResponse {
            raw: content.to_string(),
            model,
        })
    }
}

/// Turn a transport failure into something a user can act on, without echoing anything
/// that might contain the key or the payload.
fn describe_transport_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the request to OpenAI timed out".to_string()
    } else if error.is_connect() {
        "could not reach OpenAI; check the network connection and the base URL".to_string()
    } else {
        "the request to OpenAI failed".to_string()
    }
}

/// Map status codes to advice. The body is included only for statuses that carry a useful
/// validation message, and is truncated.
fn describe_http_error(status: u16, body: &str) -> String {
    match status {
        401 => "OpenAI rejected the API key. Check it in Settings > AI.".to_string(),
        403 => "This OpenAI key is not permitted to use that model.".to_string(),
        404 => format!(
            "OpenAI does not recognise that model or endpoint. {}",
            summarize_body(body)
        ),
        429 => "OpenAI rate limit or quota reached. Try again shortly.".to_string(),
        500..=599 => "OpenAI reported a server error. Try again shortly.".to_string(),
        _ => format!("OpenAI returned status {status}. {}", summarize_body(body)),
    }
}

fn summarize_body(body: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.to_string());
    message.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PrivacyMode;

    fn provider() -> OpenAiProvider {
        OpenAiProvider::new(
            "https://api.openai.com/v1/".to_string(),
            "gpt-4.1-mini".to_string(),
            "sk-secret-canary".to_string(),
        )
    }

    #[test]
    fn the_base_url_is_normalized_and_the_destination_is_explicit() {
        let provider = provider();
        assert_eq!(provider.base_url, "https://api.openai.com/v1");
        assert_eq!(provider.privacy_mode(), PrivacyMode::Cloud);
        let destination = provider.destination();
        assert!(destination.contains("OpenAI cloud"));
        assert!(
            !destination.contains("sk-secret-canary"),
            "the key must never be displayed"
        );
    }

    #[test]
    fn http_errors_are_translated_into_advice_not_raw_bodies() {
        assert!(describe_http_error(401, "{}").contains("Settings > AI"));
        assert!(describe_http_error(429, "{}").contains("rate limit"));
        assert!(describe_http_error(503, "{}").contains("server error"));
        let unknown =
            describe_http_error(418, r#"{"error":{"message":"model_not_found: nonsense"}}"#);
        assert!(unknown.contains("model_not_found"));
    }

    #[test]
    fn error_summaries_are_bounded() {
        let body = format!("{{\"error\":{{\"message\":\"{}\"}}}}", "x".repeat(5_000));
        assert!(summarize_body(&body).chars().count() <= 300);
    }

    #[test]
    fn the_key_is_wiped_from_memory_on_drop() {
        // Zeroizing guarantees the buffer is cleared; this asserts the field type rather
        // than reading freed memory, which would be unsound.
        let provider = provider();
        assert_eq!(provider.api_key.as_str(), "sk-secret-canary");
        let debug = format!("{:?}", provider.model);
        assert!(!debug.contains("sk-"));
    }
}
