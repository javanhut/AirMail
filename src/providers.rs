//! Built-in server settings for the providers people actually use.
//!
//! Setup asks for a provider and an address; everything an IMAP/SMTP client
//! needs is filled in from this table, so nobody has to know what a port is.

use crate::models::{AccountConfig, OAuthProvider, SmtpSecurity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provider {
    pub name: &'static str,
    /// Domains that pick this provider automatically from the address.
    pub domains: &'static [&'static str],
    pub imap_host: &'static str,
    pub imap_port: u16,
    pub smtp_host: &'static str,
    pub smtp_port: u16,
    pub smtp_security: SmtpSecurity,
    /// Shown under the password field when the provider wants something other
    /// than the password used to sign in on the web.
    pub hint: Option<&'static str>,
    /// Browser sign-in, when the provider offers it and a client ID for it is
    /// configured. The password hint is for when it is not.
    pub oauth: Option<OAuthProvider>,
}

const APP_PASSWORD: &str = "This provider rejects your normal password. Create an app password in your account's security settings and paste it here.";

pub const PROVIDERS: &[Provider] = &[
    Provider {
        name: "Gmail",
        domains: &["gmail.com", "googlemail.com"],
        imap_host: "imap.gmail.com",
        imap_port: 993,
        smtp_host: "smtp.gmail.com",
        smtp_port: 465,
        smtp_security: SmtpSecurity::Tls,
        hint: Some(
            "Gmail needs an app password: Google Account → Security → 2-Step Verification → App passwords.",
        ),
        oauth: Some(OAuthProvider::Google),
    },
    Provider {
        name: "Outlook",
        domains: &[
            "outlook.com",
            "hotmail.com",
            "live.com",
            "msn.com",
            "passport.com",
        ],
        imap_host: "outlook.office365.com",
        imap_port: 993,
        smtp_host: "smtp.office365.com",
        smtp_port: 587,
        smtp_security: SmtpSecurity::StartTls,
        hint: Some(
            "If your Microsoft account uses two-step verification, create an app password instead of your sign-in password.",
        ),
        oauth: Some(OAuthProvider::Microsoft),
    },
    Provider {
        name: "Yahoo",
        domains: &[
            "yahoo.com",
            "yahoo.co.uk",
            "yahoo.co.jp",
            "ymail.com",
            "rocketmail.com",
        ],
        imap_host: "imap.mail.yahoo.com",
        imap_port: 993,
        smtp_host: "smtp.mail.yahoo.com",
        smtp_port: 465,
        smtp_security: SmtpSecurity::Tls,
        hint: Some(APP_PASSWORD),
        oauth: None,
    },
    Provider {
        name: "iCloud",
        domains: &["icloud.com", "me.com", "mac.com"],
        imap_host: "imap.mail.me.com",
        imap_port: 993,
        smtp_host: "smtp.mail.me.com",
        smtp_port: 587,
        smtp_security: SmtpSecurity::StartTls,
        hint: Some(
            "iCloud only accepts app-specific passwords: appleid.apple.com → Sign-In and Security → App-Specific Passwords.",
        ),
        oauth: None,
    },
    Provider {
        name: "Fastmail",
        domains: &["fastmail.com", "fastmail.fm"],
        imap_host: "imap.fastmail.com",
        imap_port: 993,
        smtp_host: "smtp.fastmail.com",
        smtp_port: 465,
        smtp_security: SmtpSecurity::Tls,
        hint: Some(APP_PASSWORD),
        oauth: None,
    },
    Provider {
        name: "Zoho",
        domains: &["zoho.com", "zohomail.com", "zoho.eu"],
        imap_host: "imap.zoho.com",
        imap_port: 993,
        smtp_host: "smtp.zoho.com",
        smtp_port: 465,
        smtp_security: SmtpSecurity::Tls,
        hint: None,
        oauth: None,
    },
    Provider {
        name: "AOL",
        domains: &["aol.com", "aim.com"],
        imap_host: "imap.aol.com",
        imap_port: 993,
        smtp_host: "smtp.aol.com",
        smtp_port: 465,
        smtp_security: SmtpSecurity::Tls,
        hint: Some(APP_PASSWORD),
        oauth: None,
    },
    Provider {
        name: "GMX",
        domains: &["gmx.com", "gmx.net", "gmx.de", "gmx.at", "gmx.ch"],
        imap_host: "imap.gmx.com",
        imap_port: 993,
        smtp_host: "mail.gmx.com",
        smtp_port: 465,
        smtp_security: SmtpSecurity::Tls,
        hint: Some(
            "GMX needs IMAP switched on first: web mail → Settings → POP3 & IMAP → enable access.",
        ),
        oauth: None,
    },
];

/// The domain part of an address, lowercased. `None` unless the input is a
/// single address with exactly one `@` and something on both sides of it.
pub fn domain_of(email: &str) -> Option<String> {
    let email = email.trim();
    let (local, domain) = email.split_once('@')?;
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return None;
    }
    Some(domain.to_ascii_lowercase())
}

/// The provider that serves this address, if we ship settings for it.
pub fn for_email(email: &str) -> Option<&'static Provider> {
    let domain = domain_of(email)?;
    PROVIDERS
        .iter()
        .find(|p| p.domains.iter().any(|d| *d == domain))
}

impl Provider {
    /// Server settings for an address hosted by this provider.
    pub fn account_config(&self, email: &str) -> AccountConfig {
        AccountConfig {
            email: email.trim().to_string(),
            display_name: display_name_from(email),
            username: None,
            imap_host: self.imap_host.to_string(),
            imap_port: self.imap_port,
            smtp_host: self.smtp_host.to_string(),
            smtp_port: self.smtp_port,
            smtp_security: self.smtp_security,
            oauth: None,
        }
    }
}

/// Settings for a domain we don't ship: the near-universal `imap.`/`smtp.`
/// convention on the standard TLS ports. Setup shows these so they can be
/// corrected before saving.
pub fn guess_config(email: &str) -> AccountConfig {
    let domain = domain_of(email).unwrap_or_default();
    AccountConfig {
        email: email.trim().to_string(),
        display_name: display_name_from(email),
        username: None,
        imap_host: if domain.is_empty() {
            String::new()
        } else {
            format!("imap.{domain}")
        },
        imap_port: 993,
        smtp_host: if domain.is_empty() {
            String::new()
        } else {
            format!("smtp.{domain}")
        },
        smtp_port: 465,
        smtp_security: SmtpSecurity::Tls,
        oauth: None,
    }
}

/// A readable From name derived from the address, so setup doesn't have to ask
/// for one: `ada.lovelace@…` becomes `Ada Lovelace`.
fn display_name_from(email: &str) -> Option<String> {
    let local = email.trim().split('@').next()?;
    let name = local
        .split(['.', '_', '-', '+'])
        .filter(|part| !part.is_empty() && part.chars().any(char::is_alphabetic))
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    (!name.is_empty()).then_some(name)
}
