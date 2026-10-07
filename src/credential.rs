//! What a pasted secret is (port of `src/shared/credential.ts`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialKind {
    ApiKey,
    /// Serialises as `oauthToken`.
    OauthToken,
}

/// What a pasted secret is. Long-lived Claude OAuth tokens (from `claude setup-token`) start with
/// `sk-ant-oat`; anything else is treated as an Anthropic API key.
pub fn credential_kind(secret: &str) -> CredentialKind {
    if secret.starts_with("sk-ant-oat") {
        CredentialKind::OauthToken
    } else {
        CredentialKind::ApiKey
    }
}

/// The environment variable Claude Code reads for this kind of credential.
pub fn credential_env(secret: &str) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = HashMap::new();
    let key = match credential_kind(secret) {
        CredentialKind::OauthToken => "CLAUDE_CODE_OAUTH_TOKEN",
        CredentialKind::ApiKey => "ANTHROPIC_API_KEY",
    };
    env.insert(key.to_string(), secret.to_string());
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_oauth_tokens_by_their_prefix() {
        assert_eq!(
            credential_kind("sk-ant-oat01-abc"),
            CredentialKind::OauthToken
        );
    }

    #[test]
    fn treats_everything_else_as_an_api_key() {
        assert_eq!(credential_kind("sk-ant-api03-abc"), CredentialKind::ApiKey);
        assert_eq!(credential_kind("whatever"), CredentialKind::ApiKey);
    }

    #[test]
    fn env_sets_the_oauth_variable_for_a_token_and_only_that_one() {
        let mut expected: HashMap<String, String> = HashMap::new();
        expected.insert(
            "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
            "sk-ant-oat01-abc".to_string(),
        );
        assert_eq!(credential_env("sk-ant-oat01-abc"), expected);
    }

    #[test]
    fn env_sets_the_api_key_variable_for_a_key_and_only_that_one() {
        let mut expected: HashMap<String, String> = HashMap::new();
        expected.insert(
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api03-abc".to_string(),
        );
        assert_eq!(credential_env("sk-ant-api03-abc"), expected);
    }

    #[test]
    fn kind_serialises_like_the_ts_strings() {
        assert_eq!(
            serde_json::to_string(&CredentialKind::OauthToken).unwrap(),
            "\"oauthToken\""
        );
        assert_eq!(
            serde_json::to_string(&CredentialKind::ApiKey).unwrap(),
            "\"apiKey\""
        );
    }
}
