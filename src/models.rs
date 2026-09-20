use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Static connection settings for one email account, persisted as TOML.
/// The password is never stored here — it lives in the OS keyring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    pub imap_host: String,
    #[serde(default = "default_imap_port")]
    pub imap_port: u16,
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
}

fn default_imap_port() -> u16 {
    993
}

fn default_smtp_port() -> u16 {
    465
}

impl AccountConfig {
    pub fn imap_username(&self) -> &str {
        self.username.as_deref().unwrap_or(&self.email)
    }
}

/// An account as stored in the local database.
#[derive(Debug, Clone)]
pub struct Account {
    pub id: i64,
    pub config: AccountConfig,
    /// Name of the folder used for sent mail on this server, if found.
    pub sent_folder: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    pub name: String,
    pub uid_validity: i64,
    pub last_seen_uid: u32,
}

/// One row in a message list.
#[derive(Debug, Clone)]
pub struct MessageSummary {
    pub id: i64,
    pub account_email: String,
    pub folder_name: String,
    pub uid: u32,
    pub subject: String,
    pub from: String,
    pub date: Option<DateTime<Utc>>,
    pub seen: bool,
    pub has_attachments: bool,
}

/// Full message for the reading pane.
#[derive(Debug, Clone)]
pub struct MessageDetail {
    pub summary: MessageSummary,
    pub to: String,
    pub body_text: String,
    pub body_html: String,
}
