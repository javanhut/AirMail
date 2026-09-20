use anyhow::{Context, Result};
use lettre::message::{Mailbox as LettreMailbox, MessageBuilder};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::models::AccountConfig;

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
    let transport = transport(cfg, password);
    transport
        .send(message)
        .await
        .context("SMTP send failed")?;
    Ok(SentMessage { raw })
}

/// Build an SMTP transport: implicit TLS (SMTPS) on the configured port.
fn transport(cfg: &AccountConfig, password: &str) -> AsyncSmtpTransport<Tokio1Executor> {
    AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.smtp_host)
        .expect("invalid SMTP host")
        .port(cfg.smtp_port)
        .credentials(Credentials::new(cfg.imap_username().to_string(), password.to_string()))
        .build()
}

/// Check IMAP login and SMTP connectivity for every configured account.
pub async fn check_account(cfg: &AccountConfig, password: &str) -> Result<()> {
    transport(cfg, password)
        .test_connection()
        .await
        .context("SMTP connection/auth failed")?;
    Ok(())
}
