//! Check an API key or OAuth token with Anthropic before saving it. Port of
//! `src/main/apiKeyCheck.ts`.

use std::time::Duration;

use crate::credential::{credential_kind, CredentialKind};
use crate::host::secret::SecretStore;

/// What the check needs from an HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    pub status: u16,
    /// The response body, when the fetcher read it (the real one does for a 401).
    pub body: Option<String>,
}

/// Makes a GET request with the given headers. Injected so tests do not hit the network.
/// Returns `Err` for anything that is not an HTTP response (offline, timeout).
pub type FetchStatus<'a> = &'a dyn Fn(&str, &[(String, String)]) -> Result<FetchResponse, String>;

const MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const TIMEOUT_SECS: u64 = 10;

/// The real fetcher, over `ureq`. Blocking; call it from a worker thread.
pub fn ureq_fetch(url: &str, headers: &[(String, String)]) -> Result<FetchResponse, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_SECS)))
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let mut response = request.call().map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = if status == 401 {
        response.body_mut().read_to_string().ok()
    } else {
        None
    };
    Ok(FetchResponse { status, body })
}

fn header(name: &str, value: &str) -> (String, String) {
    (name.to_string(), value.to_string())
}

/// A message if Anthropic rejects the key, otherwise `None`. An unreachable API or an unexpected
/// status is not treated as a rejection, so a key can still be saved while offline.
pub fn check_api_key(key: &str, fetch: FetchStatus<'_>) -> Option<String> {
    let headers = vec![
        header("x-api-key", key),
        header("anthropic-version", "2023-06-01"),
    ];
    match fetch(MODELS_URL, &headers) {
        Ok(response) if response.status == 401 || response.status == 403 => {
            Some("Anthropic rejected that API key.".to_string())
        }
        _ => None,
    }
}

/// Whether a 401's error message says the token itself is bad (as opposed to, say, a missing
/// scope): it mentions "invalid", "expired" or "revoked", ignoring case.
fn says_bad_token(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("invalid") || lower.contains("expired") || lower.contains("revoked")
}

/// A message if Anthropic says the OAuth token is invalid or expired, otherwise `None`. Unlike an
/// API key, a valid long-lived token (from `claude setup-token`) is scoped for inference, so the
/// models endpoint might answer it with something other than 200 (this is unconfirmed against a
/// real token). So only a 401 whose error says the token is bad counts as a rejection; every other
/// outcome, including network failures, lets it be saved.
pub fn check_oauth_token(token: &str, fetch: FetchStatus<'_>) -> Option<String> {
    let headers = vec![
        header("authorization", &format!("Bearer {}", token)),
        header("anthropic-beta", "oauth-2025-04-20"),
        header("anthropic-version", "2023-06-01"),
    ];
    let response = match fetch(MODELS_URL, &headers) {
        Ok(response) => response,
        Err(_) => return None,
    };
    if response.status != 401 {
        return None;
    }
    let body = response.body.unwrap_or_default();
    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(_) => return None,
    };
    match parsed
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        Some(message) if says_bad_token(message) => {
            Some("Anthropic rejected that OAuth token. It may be invalid or expired.".to_string())
        }
        _ => None,
    }
}

/// Check an API key or OAuth token with Anthropic and return the trimmed value. Checking first
/// matters: the agent treats a bad key as retryable and keeps retrying for minutes, which looks like
/// a hang, so the user hears about it up front. OAuth tokens get the narrower check above.
pub fn validate_api_key(key: &str, fetch: FetchStatus<'_>) -> Result<String, String> {
    let trimmed = key.trim().to_string();
    if trimmed.is_empty() {
        return Err("The API key is empty".to_string());
    }
    let problem = match credential_kind(&trimmed) {
        CredentialKind::ApiKey => check_api_key(&trimmed, fetch),
        CredentialKind::OauthToken => check_oauth_token(&trimmed, fetch),
    };
    match problem {
        Some(message) => Err(message),
        None => Ok(trimmed),
    }
}

/// Validate and save an API key or OAuth token under `name` in the secret store.
/// `before_save` runs after validation and before storing; return `Err` to abort (for example a
/// declined prompt).
pub fn save_api_key(
    store: &dyn SecretStore,
    name: &str,
    key: &str,
    fetch: FetchStatus<'_>,
    before_save: Option<&dyn Fn() -> Result<(), String>>,
) -> Result<(), String> {
    let trimmed = validate_api_key(key, fetch)?;
    if let Some(hook) = before_save {
        hook()?;
    }
    store.set(name, &trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::secret::MemorySecretStore;
    use std::cell::{Cell, RefCell};

    type Call = (String, Vec<(String, String)>);

    fn respond(status: u16) -> impl Fn(&str, &[(String, String)]) -> Result<FetchResponse, String> {
        move |_url: &str, _headers: &[(String, String)]| -> Result<FetchResponse, String> {
            Ok(FetchResponse { status, body: None })
        }
    }

    fn respond_with(
        status: u16,
        body: &str,
    ) -> impl Fn(&str, &[(String, String)]) -> Result<FetchResponse, String> {
        let body = body.to_string();
        move |_url: &str, _headers: &[(String, String)]| -> Result<FetchResponse, String> {
            Ok(FetchResponse {
                status,
                body: Some(body.clone()),
            })
        }
    }

    fn offline() -> impl Fn(&str, &[(String, String)]) -> Result<FetchResponse, String> {
        |_url: &str, _headers: &[(String, String)]| -> Result<FetchResponse, String> {
            Err("offline".to_string())
        }
    }

    fn auth_error(message: &str) -> String {
        serde_json::json!({
            "type": "error",
            "error": { "type": "authentication_error", "message": message }
        })
        .to_string()
    }

    fn has_header(headers: &[(String, String)], name: &str, value: &str) -> bool {
        headers.iter().any(|(n, v)| n == name && v == value)
    }

    #[test]
    fn check_api_key_sends_the_key_to_the_models_endpoint() {
        let calls: RefCell<Vec<Call>> = RefCell::new(Vec::new());
        let fetch = |url: &str, headers: &[(String, String)]| -> Result<FetchResponse, String> {
            calls.borrow_mut().push((url.to_string(), headers.to_vec()));
            Ok(FetchResponse {
                status: 200,
                body: None,
            })
        };
        assert_eq!(check_api_key("sk-1", &fetch), None);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "https://api.anthropic.com/v1/models");
        assert!(has_header(&calls[0].1, "x-api-key", "sk-1"));
    }

    #[test]
    fn check_api_key_reports_a_rejected_key() {
        assert!(check_api_key("k", &respond(401))
            .unwrap()
            .contains("rejected"));
        assert!(check_api_key("k", &respond(403))
            .unwrap()
            .contains("rejected"));
    }

    #[test]
    fn check_api_key_does_not_blame_the_key_for_other_statuses_or_network_failures() {
        assert_eq!(check_api_key("k", &respond(500)), None);
        assert_eq!(check_api_key("k", &respond(429)), None);
        assert_eq!(check_api_key("k", &offline()), None);
    }

    #[test]
    fn check_oauth_token_sends_a_bearer_token_with_the_oauth_beta_header() {
        let calls: RefCell<Vec<Call>> = RefCell::new(Vec::new());
        let fetch = |url: &str, headers: &[(String, String)]| -> Result<FetchResponse, String> {
            calls.borrow_mut().push((url.to_string(), headers.to_vec()));
            Ok(FetchResponse {
                status: 200,
                body: None,
            })
        };
        assert_eq!(check_oauth_token("sk-ant-oat01-x", &fetch), None);
        let calls = calls.borrow();
        let headers = &calls[0].1;
        assert!(has_header(
            headers,
            "authorization",
            "Bearer sk-ant-oat01-x"
        ));
        assert!(has_header(headers, "anthropic-beta", "oauth-2025-04-20"));
        assert!(!headers.iter().any(|(n, _)| n == "x-api-key"));
    }

    #[test]
    fn check_oauth_token_rejects_a_401_that_says_the_token_is_invalid_or_expired() {
        let invalid = respond_with(401, &auth_error("OAuth access token is invalid."));
        assert!(check_oauth_token("t", &invalid)
            .unwrap()
            .contains("rejected"));
        let expired = respond_with(401, &auth_error("OAuth token has expired."));
        assert!(check_oauth_token("t", &expired)
            .unwrap()
            .contains("expired"));
    }

    #[test]
    fn check_oauth_token_does_not_reject_a_401_for_another_reason_or_an_unreadable_one() {
        let missing_scope = respond_with(401, &auth_error("Missing scope"));
        assert_eq!(check_oauth_token("t", &missing_scope), None);
        assert_eq!(check_oauth_token("t", &respond_with(401, "{}")), None);
        assert_eq!(
            check_oauth_token("t", &respond_with(401, "{\"error\":{\"message\":5}}")),
            None
        );
        assert_eq!(check_oauth_token("t", &respond_with(401, "not json")), None);
        assert_eq!(check_oauth_token("t", &respond(401)), None);
    }

    #[test]
    fn check_oauth_token_lets_every_other_outcome_through() {
        for status in [200u16, 403, 429, 500] {
            let fetch = respond_with(status, &auth_error("invalid"));
            assert_eq!(check_oauth_token("t", &fetch), None);
        }
        assert_eq!(check_oauth_token("t", &offline()), None);
    }

    #[test]
    fn save_saves_a_trimmed_key_that_passes_the_check() {
        let store = MemorySecretStore::new();
        save_api_key(&store, "p", "  sk-1\n", &respond(200), None).unwrap();
        assert_eq!(store.get("p"), Ok(Some("sk-1".to_string())));
    }

    #[test]
    fn save_saves_when_the_check_cannot_reach_the_api() {
        let store = MemorySecretStore::new();
        save_api_key(&store, "p", "sk-1", &offline(), None).unwrap();
        assert_eq!(store.get("p"), Ok(Some("sk-1".to_string())));
    }

    #[test]
    fn save_saves_an_oauth_token_that_is_not_rejected() {
        let store = MemorySecretStore::new();
        save_api_key(&store, "p", "sk-ant-oat01-abc", &respond(200), None).unwrap();
        assert_eq!(store.get("p"), Ok(Some("sk-ant-oat01-abc".to_string())));
    }

    #[test]
    fn save_does_not_save_an_oauth_token_anthropic_says_is_invalid() {
        let store = MemorySecretStore::new();
        let bad = respond_with(401, &auth_error("OAuth access token is invalid."));
        let err = save_api_key(&store, "p", "sk-ant-oat01-abc", &bad, None).unwrap_err();
        assert!(err.contains("OAuth token"));
        assert!(store.is_empty());
    }

    #[test]
    fn save_does_not_save_a_rejected_key() {
        let store = MemorySecretStore::new();
        let err = save_api_key(&store, "p", "bad", &respond(401), None).unwrap_err();
        assert!(err.contains("rejected"));
        assert!(store.is_empty());
    }

    #[test]
    fn save_runs_the_pre_save_hook_after_validation_and_does_not_save_if_it_fails() {
        let store = MemorySecretStore::new();
        let order: RefCell<Vec<&str>> = RefCell::new(Vec::new());
        let hook = || -> Result<(), String> {
            order.borrow_mut().push("hook");
            Err("declined".to_string())
        };
        let fetch = |_url: &str, _headers: &[(String, String)]| -> Result<FetchResponse, String> {
            order.borrow_mut().push("check");
            Ok(FetchResponse {
                status: 200,
                body: None,
            })
        };
        let err = save_api_key(&store, "p", "sk-1", &fetch, Some(&hook)).unwrap_err();
        assert!(err.contains("declined"));
        assert_eq!(*order.borrow(), vec!["check", "hook"]);
        assert!(store.is_empty());
    }

    #[test]
    fn save_does_not_run_the_pre_save_hook_for_a_rejected_key() {
        let store = MemorySecretStore::new();
        let ran = Cell::new(false);
        let hook = || -> Result<(), String> {
            ran.set(true);
            Ok(())
        };
        let err = save_api_key(&store, "p", "bad", &respond(401), Some(&hook)).unwrap_err();
        assert!(err.contains("rejected"));
        assert!(!ran.get());
    }

    #[test]
    fn save_saves_after_the_pre_save_hook_completes() {
        let store = MemorySecretStore::new();
        let runs = Cell::new(0);
        let hook = || -> Result<(), String> {
            runs.set(runs.get() + 1);
            Ok(())
        };
        save_api_key(&store, "p", "sk-1", &respond(200), Some(&hook)).unwrap();
        assert_eq!(runs.get(), 1);
        assert_eq!(store.get("p"), Ok(Some("sk-1".to_string())));
    }

    #[test]
    fn save_does_not_save_an_empty_key_or_call_the_api_for_it() {
        let store = MemorySecretStore::new();
        let calls = Cell::new(0);
        let fetch = |_url: &str, _headers: &[(String, String)]| -> Result<FetchResponse, String> {
            calls.set(calls.get() + 1);
            Ok(FetchResponse {
                status: 200,
                body: None,
            })
        };
        let err = save_api_key(&store, "p", "   ", &fetch, None).unwrap_err();
        assert!(err.contains("empty"));
        assert_eq!(calls.get(), 0);
        assert!(store.is_empty());
    }
}
