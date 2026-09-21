use anyhow::{Context, Result};
use lettre::message::{Mailbox as LettreMailbox, MessageBuilder};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::models::{AccountConfig, SmtpSecurity};

pub struct SentMessage {
    /// Raw RFC822 of the sent message, for IMAP APPEND to Sent.
    pub raw: Vec<u8>,
}

/// Send a plain-text message through the account's SMTP server.
pub async fn send(
    cfg: &AccountConfig,
    password: &str,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<SentMessage> {
    let from = LettreMailbox::new(
        cfg.display_name.clone(),
        cfg.email
            .parse()
            .map_err(|_| anyhow::anyhow!("account email {:?} is not a valid address", cfg.email))?,
    );
    let message: Message = MessageBuilder::new()
        .from(from)
        .to(to
            .parse()
            .map_err(|_| anyhow::anyhow!("recipient {to:?} is not a valid address"))?)
        .subject(subject)
        .body(body.to_string())?;

    let raw = message.formatted().to_vec();
    transport(cfg, password)?
        .send(message)
        .await
        .context("SMTP send failed")?;
    Ok(SentMessage { raw })
}

/// Build an SMTP transport matching the account's encryption: implicit TLS
/// (SMTPS, port 465) or a plain connection upgraded with STARTTLS (port 587).
/// Office 365 and iCloud only offer the latter.
fn transport(cfg: &AccountConfig, password: &str) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let builder = match cfg.smtp_security {
        SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.smtp_host),
        SmtpSecurity::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.smtp_host)
        }
    }
    .with_context(|| format!("invalid SMTP host {:?}", cfg.smtp_host))?;

    Ok(builder
        .port(cfg.smtp_port)
        .credentials(Credentials::new(
            cfg.imap_username().to_string(),
            password.to_string(),
        ))
        .build())
}

/// Check IMAP login and SMTP connectivity for every configured account.
pub async fn check_account(cfg: &AccountConfig, password: &str) -> Result<()> {
    transport(cfg, password)?
        .test_connection()
        .await
        .context("SMTP connection/auth failed")?;
    Ok(())
}
