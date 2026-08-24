//! Local inference provider (Ollama and compatible APIs).
//!
//! Talks to `/api/chat` with `stream: false` and `format` set to the JSON schema, which
//! recent Ollama builds honour; older builds fall back to plain JSON mode, and the strict
//! validation in [`super::schema`] catches whatever comes back either way.
//!
//! Nothing here leaves the machine unless the user has pointed the endpoint at another
//! host, which the UI shows in full before every call.

use serde_json::json;

use crate::error::{Error, Result};
use crate::models::AiProviderKind;

use super::provider::{AiProvider, ProviderRequest, ProviderResponse};

pub struct LocalProvider {
    endpoint: String,
    model: String,
}

impl LocalProvider {
    pub fn new(endpoint: String, model: String) -> Self {
        LocalProvider {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            model,
        }
    }

    /// True when the endpoint really is on this machine. Shown in the privacy banner so a
    /// misconfigured "local" endpoint pointing at a remote host cannot masquerade as
    /// private.
    pub fn is_loopback(&self) -> bool {
        let authority = self
            .endpoint
            .split_once("//")
            .map(|(_, rest)| rest)
            .unwrap_or(&self.endpoint)
            .split('/')
            .next()
            .unwrap_or("");
        // A bracketed IPv6 authority keeps its own colons; anything else splits on the
        // port separator.
        let host = match authority.strip_prefix('[') {
            Some(rest) => rest.split(']').next().unwrap_or(""),
            None => authority.split(':').next().unwrap_or(""),
        };
        if host == "localhost" || host == "::1" {
            return true;
        }
        host.parse::<std::net::IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
    }
}

impl AiProvider for LocalProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Local
    }

    fn model(&self) -> String {
        self.model.clone()
    }

    fn destination(&self) -> String {
        if self.is_loopback() {
            format!(
                "Local inference on this machine ({}), model {}",
                self.endpoint, self.model
            )
        } else {
            format!(
                "Local-API inference on another host ({}), model {}. Text leaves this machine.",
                self.endpoint, self.model
            )
        }
    }

    fn complete(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(request.timeout_secs))
            .build()
            .map_err(|e| Error::AiProvider(format!("could not create the HTTP client: {e}")))?;

        let body = json!({
            "model": self.model,
            "stream": false,
            "format": request.schema.json_schema(),
            "options": {
                "temperature": request.temperature,
                "num_predict": request.max_output_tokens,
            },
            "messages": [
                { "role": "system", "content": request.system },
                { "role": "user", "content": request.user }
            ]
        });

        let response = client
            .post(format!("{}/api/chat", self.endpoint))
            .json(&body)
            .send()
            .map_err(|e| Error::AiProvider(describe_transport_error(&e, &self.endpoint)))?;

        let status = response.status();
        let text = response
            .text()
            .map_err(|e| Error::AiProvider(format!("could not read the response: {e}")))?;

        if !status.is_success() {
            return Err(Error::AiProvider(describe_http_error(
                status.as_u16(),
                &text,
                &self.model,
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
            Error::AiProvider("the local endpoint returned a response that is not JSON".into())
        })?;

        // Ollama's chat shape, with the OpenAI-compatible shape as a fallback so a
        // llama.cpp or vLLM server behind the same setting also works.
        let content = parsed["message"]["content"]
            .as_str()
            .or_else(|| parsed["choices"][0]["message"]["content"].as_str())
            .or_else(|| parsed["response"].as_str())
            .ok_or_else(|| {
                Error::AiProvider("the local endpoint returned no message content".into())
            })?;

        let model = parsed["model"].as_str().unwrap_or(&self.model).to_string();
        Ok(ProviderResponse {
            raw: content.to_string(),
            model,
        })
    }
}

fn describe_transport_error(error: &reqwest::Error, endpoint: &str) -> String {
    if error.is_timeout() {
        "the local model did not respond in time; a larger model may need a longer timeout"
            .to_string()
    } else if error.is_connect() {
        format!(
            "could not reach the local AI endpoint at {endpoint}. Is Ollama running? \
             Start it with `ollama serve`."
        )
    } else {
        format!("the request to {endpoint} failed")
    }
}

fn describe_http_error(status: u16, body: &str, model: &str) -> String {
    if status == 404 && body.contains("model") {
        return format!("the local endpoint does not have the model `{model}`. Pull it with `ollama pull {model}`.");
    }
    match status {
        404 => "the local endpoint has no /api/chat route; check the endpoint URL".to_string(),
        400 => format!(
            "the local endpoint rejected the request: {}",
            summarize(body)
        ),
        500..=599 => format!("the local endpoint reported an error: {}", summarize(body)),
        _ => format!("the local endpoint returned status {status}"),
    }
}

fn summarize(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.to_string())
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PrivacyMode;

    #[test]
    fn loopback_endpoints_are_recognised() {
        for endpoint in [
            "http://127.0.0.1:11434",
            "http://localhost:11434/",
            "http://[::1]:11434",
        ] {
            let provider = LocalProvider::new(endpoint.to_string(), "llama3.1:8b".to_string());
            assert!(provider.is_loopback(), "{endpoint} should count as local");
            assert!(provider.destination().contains("on this machine"));
        }
    }

    #[test]
    fn a_remote_endpoint_is_not_described_as_private() {
        let provider =
            LocalProvider::new("http://10.0.0.20:11434".to_string(), "qwen2.5".to_string());
        assert!(!provider.is_loopback());
        let destination = provider.destination();
        assert!(
            destination.contains("leaves this machine"),
            "got {destination}"
        );
        // The privacy mode is still Local, but the destination string tells the truth.
        assert_eq!(provider.privacy_mode(), PrivacyMode::Local);
    }

    #[test]
    fn a_missing_model_produces_actionable_advice() {
        let message = describe_http_error(404, r#"{"error":"model 'x' not found"}"#, "llama3.1:8b");
        assert!(message.contains("ollama pull llama3.1:8b"), "got {message}");
    }

    #[test]
    fn trailing_slashes_do_not_produce_double_slashes() {
        let provider = LocalProvider::new("http://127.0.0.1:11434/".to_string(), "m".to_string());
        assert_eq!(provider.endpoint, "http://127.0.0.1:11434");
    }
}
