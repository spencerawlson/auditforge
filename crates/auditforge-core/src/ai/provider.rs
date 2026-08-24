//! The provider abstraction.
//!
//! Everything above this file talks to [`AiProvider`] and never to a vendor. Adding a
//! provider means implementing one trait with one method; nothing in the report, finding
//! or review code changes.

use crate::error::{Error, Result};
use crate::models::{AiProviderKind, AiSettings, PrivacyMode};

use super::schema::ResponseSchema;

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub system: String,
    pub user: String,
    pub schema: ResponseSchema,
    pub temperature: f32,
    pub max_output_tokens: u32,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    /// The raw body the model produced. Validated by `schema` before anything else sees
    /// it.
    pub raw: String,
    pub model: String,
}

pub trait AiProvider: Send + Sync {
    fn kind(&self) -> AiProviderKind;
    fn model(&self) -> String;

    fn privacy_mode(&self) -> PrivacyMode {
        match self.kind() {
            AiProviderKind::OpenAi => PrivacyMode::Cloud,
            AiProviderKind::Local => PrivacyMode::Local,
            AiProviderKind::Disabled => PrivacyMode::Offline,
        }
    }

    /// Human-readable description of where text goes, shown in the UI before a call.
    fn destination(&self) -> String;

    fn complete(&self, request: &ProviderRequest) -> Result<ProviderResponse>;
}

/// The default. Refuses every request with an explanation rather than silently doing
/// nothing, so a user who expects AI help is told why there is none.
pub struct DisabledProvider;

impl AiProvider for DisabledProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Disabled
    }

    fn model(&self) -> String {
        String::new()
    }

    fn destination(&self) -> String {
        "No AI provider is configured. Nothing leaves this machine.".to_string()
    }

    fn complete(&self, _request: &ProviderRequest) -> Result<ProviderResponse> {
        Err(Error::AiPolicy(
            "AI assistance is turned off. Choose a provider in Settings > AI to enable it."
                .to_string(),
        ))
    }
}

/// Build the provider described by `settings`.
///
/// `api_key` is fetched from the OS keychain by the caller and passed in, so this module
/// never touches secret storage.
pub fn build(settings: &AiSettings, api_key: Option<String>) -> Result<Box<dyn AiProvider>> {
    match settings.provider {
        AiProviderKind::Disabled => Ok(Box::new(DisabledProvider)),
        AiProviderKind::Local => Ok(Box::new(super::local::LocalProvider::new(
            settings.local_endpoint.clone(),
            settings.local_model.clone(),
        ))),
        AiProviderKind::OpenAi => {
            // Two independent gates: a key must exist, and the user must have turned on
            // cloud processing. Selecting the provider is not by itself consent.
            if !settings.cloud_consent {
                return Err(Error::AiPolicy(
                    "Cloud AI is selected but not enabled. Turn on cloud processing in \
                     Settings > AI to allow report text to be sent to OpenAI."
                        .to_string(),
                ));
            }
            let key = api_key
                .filter(|key| !key.trim().is_empty())
                .ok_or_else(|| {
                    Error::AiPolicy(
                        "No OpenAI API key is stored. Add one in Settings > AI.".to_string(),
                    )
                })?;
            Ok(Box::new(super::openai::OpenAiProvider::new(
                settings.openai_base_url.clone(),
                settings.openai_model.clone(),
                key,
            )))
        }
    }
}

/// A provider that replays canned responses instead of calling anything.
///
/// Used by the test suite, and by the offline walkthrough in `tests/`, so the full
/// accept/reject workflow can be exercised without a network or an API key.
pub struct ScriptedProvider {
    responses: std::sync::Mutex<Vec<String>>,
    model: String,
}

impl ScriptedProvider {
    pub fn new(responses: Vec<String>) -> Self {
        // Reversed so `pop` yields them in the order given.
        let mut responses = responses;
        responses.reverse();
        ScriptedProvider {
            responses: std::sync::Mutex::new(responses),
            model: "scripted".into(),
        }
    }

    /// Always returns the same body.
    pub fn always(response: impl Into<String>) -> Self {
        ScriptedProvider {
            responses: std::sync::Mutex::new(vec![response.into()]),
            model: "scripted".into(),
        }
    }
}

impl AiProvider for ScriptedProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Local
    }

    fn model(&self) -> String {
        self.model.clone()
    }

    fn destination(&self) -> String {
        "Scripted responses. No network access.".to_string()
    }

    fn complete(&self, _request: &ProviderRequest) -> Result<ProviderResponse> {
        let mut responses = self.responses.lock().unwrap_or_else(|e| e.into_inner());
        let raw = if responses.len() > 1 {
            responses.pop().unwrap_or_default()
        } else {
            responses.last().cloned().unwrap_or_default()
        };
        Ok(ProviderResponse {
            raw,
            model: self.model.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Box<dyn AiProvider>` is not `Debug`, so `unwrap_err` is unavailable here.
    fn expect_err(result: Result<Box<dyn AiProvider>>) -> Error {
        match result {
            Err(error) => error,
            Ok(provider) => panic!("expected the build to fail, got {}", provider.kind()),
        }
    }

    fn request() -> ProviderRequest {
        ProviderRequest {
            system: "s".to_string(),
            user: "u".to_string(),
            schema: ResponseSchema::TextSuggestion,
            temperature: 0.2,
            max_output_tokens: 100,
            timeout_secs: 10,
        }
    }

    #[test]
    fn the_disabled_provider_refuses_with_an_explanation() {
        let provider = DisabledProvider;
        assert_eq!(provider.privacy_mode(), PrivacyMode::Offline);
        let err = provider.complete(&request()).unwrap_err();
        assert_eq!(err.kind(), "ai_policy");
        assert!(err.to_string().contains("Settings"), "got {err}");
    }

    #[test]
    fn disabled_is_the_default_and_builds_without_configuration() {
        let provider = build(&AiSettings::default(), None).unwrap();
        assert_eq!(provider.kind(), AiProviderKind::Disabled);
    }

    #[test]
    fn the_cloud_provider_needs_both_consent_and_a_key() {
        let mut settings = AiSettings {
            provider: AiProviderKind::OpenAi,
            ..Default::default()
        };

        let err = expect_err(build(&settings, Some("sk-test".to_string())));
        assert!(
            err.to_string().contains("not enabled"),
            "consent must be required: {err}"
        );

        settings.cloud_consent = true;
        let err = expect_err(build(&settings, None));
        assert!(
            err.to_string().contains("API key"),
            "a key must be required: {err}"
        );

        let err = expect_err(build(&settings, Some("   ".to_string())));
        assert!(
            err.to_string().contains("API key"),
            "a blank key must not count: {err}"
        );

        let provider = build(&settings, Some("sk-test".to_string())).unwrap();
        assert_eq!(provider.kind(), AiProviderKind::OpenAi);
        assert_eq!(provider.privacy_mode(), PrivacyMode::Cloud);
    }

    #[test]
    fn the_local_provider_needs_neither_consent_nor_a_key() {
        let settings = AiSettings {
            provider: AiProviderKind::Local,
            ..Default::default()
        };
        let provider = build(&settings, None).unwrap();
        assert_eq!(provider.kind(), AiProviderKind::Local);
        assert_eq!(provider.privacy_mode(), PrivacyMode::Local);
        assert!(!provider.privacy_mode().leaves_machine());
    }

    #[test]
    fn the_scripted_provider_replays_in_order_then_repeats_the_last() {
        let provider = ScriptedProvider::new(vec!["one".to_string(), "two".to_string()]);
        assert_eq!(provider.complete(&request()).unwrap().raw, "one");
        assert_eq!(provider.complete(&request()).unwrap().raw, "two");
        assert_eq!(provider.complete(&request()).unwrap().raw, "two");
    }
}
