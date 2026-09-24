use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Static connection settings for one email account, persisted as TOML.
/// The password is never stored here — it lives in HuginnKeyring.
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
    #[serde(default)]
    pub smtp_security: SmtpSecurity,
    /// Set when the account signs in through the browser. The keyring then
    /// holds a refresh token rather than a password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthProvider>,
}

/// Providers AirMail can sign in to through the browser. See `crate::oauth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthProvider {
    Google,
    Microsoft,
}

/// How the SMTP connection is encrypted. Providers split roughly in two:
/// port 465 wraps the whole connection in TLS, port 587 starts in the clear
/// and upgrades. Getting this wrong looks like a hang, not an error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmtpSecurity {
    /// Implicit TLS for the whole session (SMTPS, usually port 465).
    #[default]
    Tls,
    /// Plain connection upgraded with STARTTLS (usually port 587).
    StartTls,
}

impl SmtpSecurity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tls => "TLS",
            Self::StartTls => "STARTTLS",
        }
    }
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
    /// The IMAP `\Flagged` flag — the star in the list and reading pane.
    pub flagged: bool,
    pub has_attachments: bool,
    /// First line or so of the body, for the row's third line. Stored
    /// squashed to one line at query time so rows do not have to re-wrap it.
    pub preview: String,
}

/// Full message for the reading pane.
#[derive(Debug, Clone)]
pub struct MessageDetail {
    pub summary: MessageSummary,
    pub to: String,
    pub body_text: String,
    pub body_html: String,
}
