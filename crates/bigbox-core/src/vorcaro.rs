// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Vorcaro's Studio data model — contacts, lists, tags, campaigns, settings.
//! Pure domain types; persistence (vorcaro.toml) lives in `bigbox-config`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContactSource {
    Scraped,
    Manual,
    Imported,
}

impl Default for ContactSource {
    fn default() -> Self { ContactSource::Manual }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: Uuid,
    pub display_name: String,
    #[serde(default)]
    pub whatsapp: Option<String>,
    #[serde(default)]
    pub whatsapp_business: Option<String>,
    #[serde(default)]
    pub telegram: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: ContactSource,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactList {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub contact_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    #[serde(rename = "whatsapp_web")]
    WhatsAppWeb,
    #[serde(rename = "whatsapp_business_web")]
    WhatsAppBusinessWeb,
    #[serde(rename = "telegram")]
    Telegram,
    #[serde(rename = "whatsapp_cloud_api")]
    WhatsAppCloudApi,
}

impl Platform {
    /// Maps to the BigBox service id used in WebView labels (`svc-<id>`).
    /// Returns None for non-WebView platforms (e.g. Cloud API).
    pub fn service_id(self) -> &'static str {
        match self {
            Platform::WhatsAppWeb => "whatsapp",
            Platform::WhatsAppBusinessWeb => "whatsapp_business",
            Platform::Telegram => "telegram",
            // Cloud API has no WebView; orchestrator branches before reaching this path.
            Platform::WhatsAppCloudApi => "",
        }
    }

    pub fn is_web_driver(self) -> bool {
        !matches!(self, Platform::WhatsAppCloudApi)
    }
}

/// A single chat row pulled from a chat-service WebView's sidebar.
/// All handles are best-effort — the DOM doesn't always expose them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapedChat {
    pub name: String,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub peer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TargetSpec {
    List(Uuid),
    Tag(String),
    AdHoc(Vec<Uuid>),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CampaignStatus {
    Draft,
    Scheduled,
    Running,
    Paused,
    Done,
    Aborted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SendStatus {
    Queued,
    Sent,
    Failed,
    Skipped,
    InvalidNumber,
}

/// What a driver's `sendTo` call (or a Cloud API send) ultimately resolves to.
/// Shared by the orchestrator (engine) and the cloud sender; lives here so
/// neither has to depend on the other.
#[derive(Debug, Clone)]
pub struct SendOutcome {
    pub status: SendStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendAttempt {
    pub contact_id: Uuid,
    pub status: SendStatus,
    #[serde(default)]
    pub error: Option<String>,
    pub at: DateTime<Utc>,
}

/// Cloud-API-only: which template to send, and how to fill its body params.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemplateUsage {
    pub name: String,
    /// Language code as defined in Meta's template (e.g. "pt_BR", "en_US").
    pub language: String,
    /// One entry per `{{N}}` placeholder in the template body, in order.
    /// Each entry is a string that goes through the variable substitution
    /// pipeline (`{nome}`, `{firstname}`, …) before being sent.
    #[serde(default)]
    pub body_params: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub id: Uuid,
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub attachments: Vec<PathBuf>,
    pub targets: TargetSpec,
    pub platform: Platform,
    pub status: CampaignStatus,
    pub created_at: DateTime<Utc>,
    /// When this campaign should start sending. None = start immediately.
    #[serde(default)]
    pub scheduled_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub progress: Vec<SendAttempt>,
    /// Cloud-API-only: when set, send the named template instead of free-form
    /// text. Required for cold outreach (recipients outside the 24h window).
    #[serde(default)]
    pub template: Option<TemplateUsage>,
    /// Specific BigBox service id to drive (e.g. "whatsapp_2"). When set and
    /// platform is a WebView-based one, the orchestrator targets `svc-<id>`
    /// instead of the canonical service for the platform. Lets users with
    /// multiple WhatsApp slots pick which account sends. Ignored for Cloud API.
    #[serde(default)]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub min_delay_secs: u32,
    pub max_delay_secs: u32,
    pub daily_cap_per_platform: u32,
    pub warn_threshold: u32,
    pub auto_pause_after_consecutive_failures: u32,
    /// Max number of Failed attempts per contact before giving up. 0 = no retries.
    #[serde(default)]
    pub max_retries_per_recipient: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            min_delay_secs: 30,
            max_delay_secs: 90,
            daily_cap_per_platform: 100,
            warn_threshold: 20,
            auto_pause_after_consecutive_failures: 3,
            max_retries_per_recipient: 0,
        }
    }
}

/// Daily send count per platform, keyed by ISO date (YYYY-MM-DD).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyCap {
    /// Maps "YYYY-MM-DD" → { "whatsapp_web": count, … }
    #[serde(default)]
    pub by_date: std::collections::BTreeMap<String, std::collections::BTreeMap<String, u32>>,
}

impl DailyCap {
    pub fn today_key() -> String {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    }

    pub fn count(&self, platform: Platform) -> u32 {
        let key = Self::today_key();
        self.by_date
            .get(&key)
            .and_then(|m| m.get(platform_key(platform)))
            .copied()
            .unwrap_or(0)
    }

    pub fn increment(&mut self, platform: Platform) {
        let key = Self::today_key();
        let bucket = self.by_date.entry(key).or_default();
        *bucket.entry(platform_key(platform).to_string()).or_insert(0) += 1;

        // Garbage collect old day entries (keep last 30 days).
        if self.by_date.len() > 30 {
            let oldest: Vec<String> = self
                .by_date
                .keys()
                .take(self.by_date.len() - 30)
                .cloned()
                .collect();
            for k in oldest {
                self.by_date.remove(&k);
            }
        }
    }
}

fn platform_key(p: Platform) -> &'static str {
    match p {
        Platform::WhatsAppWeb => "whatsapp_web",
        Platform::WhatsAppBusinessWeb => "whatsapp_business_web",
        Platform::Telegram => "telegram",
        Platform::WhatsAppCloudApi => "whatsapp_cloud_api",
    }
}

/// Top-level persisted state for Vorcaro's Studio. Lives at
/// `~/.config/bigbox/vorcaro.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VorcaroState {
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub lists: Vec<ContactList>,
    #[serde(default)]
    pub campaigns: Vec<Campaign>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub daily_cap: DailyCap,
}
