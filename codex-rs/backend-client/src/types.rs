pub use codex_backend_openapi_models::models::ConfigBundleResponse;
pub use codex_backend_openapi_models::models::CreditStatusDetails;
pub use codex_backend_openapi_models::models::DeliveredConfigToml;
pub use codex_backend_openapi_models::models::DeliveredManagedLayers;
pub use codex_backend_openapi_models::models::DeliveredRequirementsToml;
pub use codex_backend_openapi_models::models::DeliveredTomlFragment;
pub use codex_backend_openapi_models::models::PlanType;
pub use codex_backend_openapi_models::models::RateLimitReachedKind;
pub use codex_backend_openapi_models::models::RateLimitStatusDetails;
pub use codex_backend_openapi_models::models::RateLimitStatusPayload;
pub use codex_backend_openapi_models::models::RateLimitWindowSnapshot;
pub use codex_backend_openapi_models::models::SpendControlLimitDetails;

use codex_protocol::protocol::RateLimitSnapshot;
use serde::Deserialize;
use serde::de::Deserializer;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RateLimitResetCreditsSummary {
    pub available_count: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RateLimitResetCreditsDetails {
    pub credits: Vec<RateLimitResetCreditDetails>,
    pub available_count: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RateLimitResetCreditDetails {
    pub id: String,
    pub reset_type: String,
    pub status: String,
    pub granted_at: String,
    pub expires_at: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RateLimitsWithResetCredits {
    pub rate_limits: Vec<RateLimitSnapshot>,
    /// Backend decision for ordinary included usage; absence is not permission to recover.
    pub ordinary_usage_allowed: Option<bool>,
    pub rate_limit_reset_credits: Option<RateLimitResetCreditsSummary>,
    pub account_id: Option<String>,
    pub user_id: Option<String>,
    pub rate_limit_upsell: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct RateLimitStatusWithResetCredits {
    #[serde(flatten)]
    pub rate_limits: RateLimitStatusPayload,
    // Capture the new optional metadata alongside the generated quota fields until the
    // backend OpenAPI export includes it. This field owns additional_rate_limits on the wire.
    pub additional_rate_limits: Option<Vec<AdditionalRateLimitWithNormalModel>>,
    pub rate_limit_reset_credits: Option<RateLimitResetCreditsSummary>,
    pub account_id: Option<String>,
    pub user_id: Option<String>,
    // Preserve the backend-owned banner contract without making optional UI data break usage.
    pub rate_limit_upsell: Option<Value>,
}

/// Additional quota details plus the normal model whose picker metadata should be used.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct AdditionalRateLimitWithNormalModel {
    #[serde(flatten)]
    pub details: codex_backend_openapi_models::models::AdditionalRateLimitDetails,
    pub normal_model_slug: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CodexWorkspaceMessagesResponse {
    #[serde(default)]
    pub messages: Vec<CodexWorkspaceMessage>,
}

/// Authenticated Codex user settings used by CLI runtime policy.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct CodexUserSettingsResponse {
    /// Server-computed effective commit-attribution policy.
    ///
    /// Older backend responses omit this field, which safely defaults to disabled.
    #[serde(default)]
    pub commit_attribution_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CodexWorkspaceMessage {
    pub message_id: String,
    pub message_type: CodexWorkspaceMessageType,
    pub message_body: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub archived_at: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsumeRateLimitResetCreditCode {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ConsumeRateLimitResetCreditResponse {
    pub code: ConsumeRateLimitResetCreditCode,
    #[serde(default)]
    pub windows_reset: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexWorkspaceMessageType {
    Headline,
    Announcement,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug)]
pub struct AccountsCheckResponse {
    pub accounts: Vec<AccountEntry>,
    pub account_ordering: Vec<String>,
    pub default_account_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountEntry {
    pub id: String,
    pub can_access_with_session: Option<bool>,
    pub sso_connection_name: Option<String>,
    #[serde(default)]
    pub is_deactivated: bool,
    #[serde(default)]
    pub is_fedramp_compliant_workspace: bool,
    /// Current subscription reported by the accounts endpoint, independent of token claims.
    #[serde(default)]
    pub plan_type: Option<codex_protocol::account::PlanType>,
    pub workspace_backend_origin: Option<String>,
    pub account_routing_override: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub profile_picture_url: Option<String>,
    #[serde(default)]
    pub structure: String,
}

#[derive(Deserialize)]
struct RawAccountsCheckResponse {
    #[serde(default)]
    accounts: RawAccounts,
    #[serde(default)]
    account_ordering: Vec<String>,
    #[serde(default)]
    default_account_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawAccounts {
    List(Vec<AccountEntry>),
    Map(HashMap<String, ChatGptAccountEntry>),
}

impl Default for RawAccounts {
    fn default() -> Self {
        Self::List(Vec::new())
    }
}

#[derive(Deserialize)]
struct ChatGptAccountEntry {
    account: ChatGptAccountInfo,
    can_access_with_session: Option<bool>,
    sso_connection_name: Option<String>,
}

#[derive(Deserialize)]
struct ChatGptAccountInfo {
    account_id: Option<String>,
    #[serde(default)]
    is_deactivated: bool,
    #[serde(default)]
    is_fedramp_compliant_workspace: bool,
    #[serde(default)]
    plan_type: Option<codex_protocol::account::PlanType>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    profile_picture_url: Option<String>,
    #[serde(default)]
    structure: String,
}

impl<'de> Deserialize<'de> for AccountsCheckResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawAccountsCheckResponse::deserialize(deserializer)?;
        let accounts = match raw.accounts {
            RawAccounts::List(accounts) => accounts,
            RawAccounts::Map(mut accounts) => raw
                .account_ordering
                .iter()
                .filter_map(|account_id| {
                    let entry = accounts.remove(account_id)?;
                    let account = entry.account;
                    Some(AccountEntry {
                        id: account.account_id?,
                        can_access_with_session: entry.can_access_with_session,
                        sso_connection_name: entry.sso_connection_name,
                        is_deactivated: account.is_deactivated,
                        is_fedramp_compliant_workspace: account.is_fedramp_compliant_workspace,
                        plan_type: account.plan_type,
                        workspace_backend_origin: None,
                        account_routing_override: None,
                        name: account.name,
                        profile_picture_url: account.profile_picture_url,
                        structure: account.structure,
                    })
                })
                .collect(),
        };
        Ok(Self {
            accounts,
            account_ordering: raw.account_ordering,
            default_account_id: raw.default_account_id,
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct TokenUsageProfile {
    pub stats: TokenUsageProfileStats,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct TokenUsageProfileStats {
    pub lifetime_tokens: Option<i64>,
    pub peak_daily_tokens: Option<i64>,
    pub longest_running_turn_sec: Option<i64>,
    pub current_streak_days: Option<i64>,
    pub longest_streak_days: Option<i64>,
    pub daily_usage_buckets: Option<Vec<TokenUsageProfileDailyBucket>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct TokenUsageProfileDailyBucket {
    pub start_date: String,
    pub tokens: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn workspace_messages_response_deserializes_messages() {
        let response: CodexWorkspaceMessagesResponse = serde_json::from_value(serde_json::json!({
            "messages": [
                {
                    "message_id": "headline-id",
                    "message_type": "headline",
                    "message_body": "Headline body",
                    "created_at": "2026-06-14T00:00:00Z",
                    "archived_at": null
                },
                {
                    "message_id": "announcement-id",
                    "message_type": "announcement",
                    "message_body": "Announcement body",
                    "created_at": "2026-06-14T01:00:00Z",
                    "archived_at": null
                },
                {
                    "message_id": "unknown-id",
                    "message_type": "unknown",
                    "message_body": "Unknown body"
                }
            ]
        }))
        .expect("workspace messages response should deserialize");

        assert_eq!(
            response,
            CodexWorkspaceMessagesResponse {
                messages: vec![
                    CodexWorkspaceMessage {
                        message_id: "headline-id".to_string(),
                        message_type: CodexWorkspaceMessageType::Headline,
                        message_body: "Headline body".to_string(),
                        created_at: Some("2026-06-14T00:00:00Z".to_string()),
                        archived_at: None,
                    },
                    CodexWorkspaceMessage {
                        message_id: "announcement-id".to_string(),
                        message_type: CodexWorkspaceMessageType::Announcement,
                        message_body: "Announcement body".to_string(),
                        created_at: Some("2026-06-14T01:00:00Z".to_string()),
                        archived_at: None,
                    },
                    CodexWorkspaceMessage {
                        message_id: "unknown-id".to_string(),
                        message_type: CodexWorkspaceMessageType::Unknown,
                        message_body: "Unknown body".to_string(),
                        created_at: None,
                        archived_at: None,
                    },
                ],
            }
        );
    }
}
