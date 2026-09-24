use std::sync::Arc;

use anyhow::{Context, Result};
use async_imap::Session;
use futures::TryStreamExt;
use rustls_pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, client::TlsStream};

use crate::models::AccountConfig;
use crate::oauth::{self, Login};

pub type ImapSession = Session<TlsStream<TcpStream>>;

/// SASL XOAUTH2. A failed attempt comes back as a challenge carrying the
/// error, which has to be answered with an empty line before the server says
/// NO -- so the payload goes out once and nothing after it.
struct XOAuth2 {
    payload: Option<String>,
}

impl async_imap::Authenticator for XOAuth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        self.payload.take().unwrap_or_default()
    }
}

/// Connect over TLS (port 993 style) and authenticate.
pub async fn connect(cfg: &AccountConfig, login: &Login) -> Result<ImapSession> {
    let tcp = TcpStream::connect((cfg.imap_host.as_str(), cfg.imap_port))
        .await
        .with_context(|| format!("connecting to {}:{}", cfg.imap_host, cfg.imap_port))?;

    let mut roots = tokio_rustls::rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = TlsConnector::from(Arc::new(
        tokio_rustls::rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ));
    let server_name = ServerName::try_from(cfg.imap_host.clone())
        .map_err(|e| anyhow::anyhow!("invalid IMAP host: {e}"))?;
    let tls_stream = tls
        .connect(server_name, tcp)
        .await
        .context("TLS handshake with IMAP server")?;

    let mut client = async_imap::Client::new(tls_stream);
    let _greeting = client
        .read_response()
        .await
        .context("reading IMAP greeting")?
        .ok_or_else(|| anyhow::anyhow!("IMAP server closed the connection (check host/port)"))?;

    let session = match login {
        Login::Password(password) => client
            .login(cfg.imap_username(), password)
            .await
            .map_err(|(err, _client)| err)
            .context("IMAP login failed (wrong credentials or auth not allowed)")?,
        Login::Bearer(token) => {
            let auth = XOAuth2 {
                payload: Some(oauth::xoauth2_payload(cfg.imap_username(), token)),
            };
            match client.authenticate("XOAUTH2", auth).await {
                Ok(session) => session,
                Err((err, _client)) => {
                    // Maybe revoked, maybe just early; either way the next
                    // attempt should ask for a fresh token.
                    oauth::forget(&cfg.email);
                    return Err(anyhow::Error::from(err).context(
                        "IMAP sign-in was refused — the account may need signing in again",
                    ));
                }
            }
        }
    };
    Ok(session)
}

/// Names of all selectable folders for the account.
pub async fn list_folders(session: &mut ImapSession) -> Result<Vec<String>> {
    let names = session
        .list(None, Some("*"))
        .await
        .context("listing folders")?
        .try_collect::<Vec<_>>()
        .await?;
    Ok(names
        .iter()
        .filter(|n| !n.name().is_empty())
        // Containers like Gmail's "[Gmail]" cannot be EXAMINEd; one of them
        // would fail the whole sync pass.
        .filter(|n| {
            !n.attributes()
                .iter()
                .any(|a| matches!(a, async_imap::types::NameAttribute::NoSelect))
        })
        .map(|n| n.name().to_string())
        .collect())
}

pub struct FetchedMessage {
    pub uid: u32,
    pub seen: bool,
    pub flagged: bool,
    pub raw: Vec<u8>,
}

/// Fetch full raw messages plus flags for the given UID set.
/// `uid_set` is an IMAP sequence set string, e.g. "42" or "100:*".
pub async fn fetch_messages(
    session: &mut ImapSession,
    folder: &str,
    uid_set: &str,
) -> Result<Vec<FetchedMessage>> {
    let _mailbox = session
        .examine(folder)
        .await
        .with_context(|| format!("cannot open folder {folder:?} — it may have been deleted"))?;
    if uid_set.is_empty() {
        return Ok(Vec::new());
    }
    let fetches = session
        .uid_fetch(uid_set, "(FLAGS BODY.PEEK[])")
        .await
        .context("UID FETCH")?
        .try_collect::<Vec<_>>()
        .await?;
    Ok(fetches
        .iter()
        .filter_map(|f| {
            let uid = f.uid?;
            let seen = f
                .flags()
                .any(|fl| matches!(fl, async_imap::types::Flag::Seen));
            let flagged = f
                .flags()
                .any(|fl| matches!(fl, async_imap::types::Flag::Flagged));
            let raw = f.body().map(<[u8]>::to_vec)?;
            Some(FetchedMessage {
                uid,
                seen,
                flagged,
                raw,
            })
        })
        .collect())
}

/// All UIDs in a folder, ascending. Used to seed the first sync.
pub async fn all_uids(session: &mut ImapSession, folder: &str) -> Result<Vec<u32>> {
    session.examine(folder).await?;
    let mut uids: Vec<u32> = session
        .uid_search("ALL")
        .await
        .context("UID SEARCH")?
        .into_iter()
        .collect();
    uids.sort_unstable();
    Ok(uids)
}

/// Append an already-sent message to the account's Sent folder.
pub async fn append_to_folder(session: &mut ImapSession, folder: &str, raw: &[u8]) -> Result<()> {
    session
        .append(folder, Some("\\Seen"), None, raw)
        .await
        .with_context(|| format!("appending to {folder:?}"))?;
    Ok(())
}
