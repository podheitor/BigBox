// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Phase F: WhatsApp Cloud API (Meta-hosted) sender.
//!
//! Unlike the DOM-injection paths (Phases C/D/E.2), this sender talks directly
//! to Meta's Graph API over HTTPS — no WebView, no scraping, no ban-risk
//! heuristics. The trade-off:
//!
//!   * Requires a verified WhatsApp Business Account + a Permanent Access
//!     Token + a phone-number-id. Setup happens outside BigBox.
//!   * Free-form text only reaches recipients with an open 24-hour session
//!     window (someone who messaged you in the last 24h). Outside that window
//!     you must use a pre-approved **template**.
//!   * No attachments support yet (Meta wants a separate /media upload first;
//!     deferred to Phase F.2).
//!
//! Secrets live in `~/.config/bigbox/vorcaro_secrets.toml` with chmod 0600 on
//! Unix. We never log the access token.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use bigbox_core::vorcaro::{SendOutcome, SendStatus};

const DEFAULT_API_VERSION: &str = "v17.0";
const GRAPH_BASE: &str = "https://graph.facebook.com";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WhatsAppCloudConfig {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub phone_number_id: String,
    #[serde(default)]
    pub business_account_id: String,
    #[serde(default)]
    pub api_version: Option<String>,
}

impl WhatsAppCloudConfig {
    pub fn is_complete(&self) -> bool {
        !self.access_token.is_empty()
            && !self.phone_number_id.is_empty()
    }
    pub fn version(&self) -> &str {
        self.api_version.as_deref().unwrap_or(DEFAULT_API_VERSION)
    }
}

pub fn secrets_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bigbox")
        .join("vorcaro_secrets.toml")
}

pub fn load_config() -> WhatsAppCloudConfig {
    let path = secrets_path();
    if !path.exists() {
        return WhatsAppCloudConfig::default();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    toml::from_str(&text).unwrap_or_default()
}

pub fn save_config(cfg: &WhatsAppCloudConfig) -> Result<(), String> {
    let path = secrets_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let text = toml::to_string_pretty(cfg).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("write: {e}"))?;
    set_owner_read_write_only(&path);
    Ok(())
}

#[cfg(unix)]
fn set_owner_read_write_only(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn set_owner_read_write_only(_: &std::path::Path) {}

// ── HTTP responses ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
    #[allow(dead_code)]
    #[serde(default)]
    pub error: Option<GraphError>,
}

#[derive(Debug, Deserialize)]
struct MessageRef {
    #[allow(dead_code)]
    pub id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GraphError {
    pub message: String,
    pub code: Option<i64>,
    pub error_subcode: Option<i64>,
    #[serde(default)]
    pub error_user_msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: GraphError,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TemplateInfo {
    pub name: String,
    pub language: String,
    pub category: String,
    pub status: String,
    /// Number of body placeholders (`{{1}}`, `{{2}}`, …) we detected.
    pub body_param_count: u32,
    /// Raw body text for the user to preview.
    #[serde(default)]
    pub body_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TemplateListPage {
    data: Vec<TemplateApi>,
    #[serde(default)]
    paging: Option<TemplatePaging>,
}

#[derive(Debug, Deserialize, Default)]
struct TemplatePaging {
    #[allow(dead_code)]
    #[serde(default)]
    cursors: Option<TemplateCursors>,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct TemplateCursors {
    #[serde(default)]
    after: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TemplateApi {
    name: String,
    language: String,
    category: String,
    status: String,
    #[serde(default)]
    components: Vec<TemplateComponent>,
}

#[derive(Debug, Deserialize)]
struct TemplateComponent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

// ── Public API ─────────────────────────────────────────────────

pub async fn verify_connection(cfg: &WhatsAppCloudConfig) -> Result<String, String> {
    if !cfg.is_complete() {
        return Err("credenciais incompletas".into());
    }
    // Cheapest endpoint that validates token + phone_number_id ownership.
    let url = format!(
        "{GRAPH_BASE}/{ver}/{id}?fields=verified_name,quality_rating",
        ver = cfg.version(),
        id = cfg.phone_number_id,
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(&cfg.access_token)
        .send()
        .await
        .map_err(|e| format!("rede: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.is_success() {
        Ok(text)
    } else {
        Err(parse_error_msg(&text, status))
    }
}

pub async fn list_templates(cfg: &WhatsAppCloudConfig) -> Result<Vec<TemplateInfo>, String> {
    if cfg.business_account_id.is_empty() {
        return Err("business_account_id é obrigatório para listar templates".into());
    }
    let mut out = vec![];
    let mut next_url = Some(format!(
        "{GRAPH_BASE}/{ver}/{waba}/message_templates?limit=100&fields=name,language,category,status,components",
        ver = cfg.version(),
        waba = cfg.business_account_id,
    ));
    let client = reqwest::Client::new();

    while let Some(url) = next_url.take() {
        let resp = client
            .get(&url)
            .bearer_auth(&cfg.access_token)
            .send()
            .await
            .map_err(|e| format!("rede: {e}"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(parse_error_msg(&text, status));
        }
        let page: TemplateListPage = serde_json::from_str(&text)
            .map_err(|e| format!("parse: {e} (body: {text})"))?;
        for t in page.data {
            let body_text = t.components.iter().find(|c| c.kind == "BODY")
                .and_then(|c| c.text.clone());
            let body_param_count = body_text
                .as_deref()
                .map(count_placeholders)
                .unwrap_or(0);
            out.push(TemplateInfo {
                name: t.name,
                language: t.language,
                category: t.category,
                status: t.status,
                body_param_count,
                body_text,
            });
        }
        next_url = page.paging.and_then(|p| p.next);
    }
    Ok(out)
}

fn count_placeholders(s: &str) -> u32 {
    // Count distinct `{{N}}` slots.
    let mut max = 0u32;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if &bytes[i..i+2] == b"{{" {
            let mut j = i + 2;
            let mut n: u32 = 0;
            let mut any = false;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                n = n.saturating_mul(10).saturating_add((bytes[j] - b'0') as u32);
                j += 1;
                any = true;
            }
            if any && j + 1 < bytes.len() && &bytes[j..j+2] == b"}}" {
                if n > max { max = n; }
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    max
}

/// Send a free-form text message. Recipient must have a 24h conversation
/// window open (i.e., must have messaged the business in the last 24h).
pub async fn send_text(cfg: &WhatsAppCloudConfig, to: &str, body: &str) -> SendOutcome {
    let url = format!(
        "{GRAPH_BASE}/{ver}/{id}/messages",
        ver = cfg.version(),
        id = cfg.phone_number_id,
    );
    let payload = serde_json::json!({
        "messaging_product": "whatsapp",
        "recipient_type": "individual",
        "to": to.trim_start_matches('+'),
        "type": "text",
        "text": { "body": body },
    });
    post_messages(cfg, &url, &payload).await
}

/// Send a templated message. Required for recipients outside the 24h window.
pub async fn send_template(
    cfg: &WhatsAppCloudConfig,
    to: &str,
    template_name: &str,
    language: &str,
    body_params: &[String],
) -> SendOutcome {
    let url = format!(
        "{GRAPH_BASE}/{ver}/{id}/messages",
        ver = cfg.version(),
        id = cfg.phone_number_id,
    );

    let params: Vec<serde_json::Value> = body_params
        .iter()
        .map(|p| serde_json::json!({ "type": "text", "text": p }))
        .collect();

    let mut components: Vec<serde_json::Value> = vec![];
    if !params.is_empty() {
        components.push(serde_json::json!({
            "type": "body",
            "parameters": params,
        }));
    }

    let payload = serde_json::json!({
        "messaging_product": "whatsapp",
        "recipient_type": "individual",
        "to": to.trim_start_matches('+'),
        "type": "template",
        "template": {
            "name": template_name,
            "language": { "code": language },
            "components": components,
        },
    });
    post_messages(cfg, &url, &payload).await
}

async fn post_messages(
    cfg: &WhatsAppCloudConfig,
    url: &str,
    body: &serde_json::Value,
) -> SendOutcome {
    let client = reqwest::Client::new();
    let resp = match client
        .post(url)
        .bearer_auth(&cfg.access_token)
        .json(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return SendOutcome {
                status: SendStatus::Failed,
                error: Some(format!("rede: {e}")),
            };
        }
    };

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if status.is_success() {
        let parsed: Result<MessagesResponse, _> = serde_json::from_str(&text);
        match parsed {
            Ok(r) if !r.messages.is_empty() => SendOutcome {
                status: SendStatus::Sent,
                error: None,
            },
            _ => SendOutcome {
                status: SendStatus::Sent,
                error: Some("resposta sem id de mensagem".into()),
            },
        }
    } else {
        // Map common Meta error codes to richer outcomes.
        let parsed: Option<GraphError> = serde_json::from_str::<ErrorEnvelope>(&text)
            .ok()
            .map(|e| e.error);
        if let Some(err) = parsed {
            let code = err.code.unwrap_or(0);
            let subcode = err.error_subcode.unwrap_or(0);
            // 131026 / 131047 / 131051 are the recipient-related errors.
            let recipient_problem =
                matches!(code, 131026 | 131047 | 131051) || matches!(subcode, 131026 | 131047 | 131051);
            let msg = err.error_user_msg.unwrap_or(err.message);
            SendOutcome {
                status: if recipient_problem {
                    SendStatus::InvalidNumber
                } else {
                    SendStatus::Failed
                },
                error: Some(format!("Meta {code}: {msg}")),
            }
        } else {
            SendOutcome {
                status: SendStatus::Failed,
                error: Some(format!("HTTP {status}: {text}")),
            }
        }
    }
}

fn parse_error_msg(text: &str, status: reqwest::StatusCode) -> String {
    match serde_json::from_str::<ErrorEnvelope>(text) {
        Ok(env) => format!("Meta {}: {}", env.error.code.unwrap_or(0), env.error.message),
        Err(_) => format!("HTTP {status}: {text}"),
    }
}
