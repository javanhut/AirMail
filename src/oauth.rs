//! Signing in through the browser (OAuth 2.0), for providers that allow it.
//!
//! Only Google and Microsoft let an ordinary desktop client do this for IMAP
//! and SMTP. Yahoo and AOL hand out mail scopes to approved partners only,
//! Fastmail likewise, and iCloud has no OAuth for mail at all; those stay on
//! app passwords.
//!
//! ## The flow
//!
//! The installed-app flow from RFC 8252: [`Pending::start`] binds a listener
//! on loopback and builds the provider's sign-in URL, the UI opens that in the
//! browser, and [`Pending::finish`] waits for the browser to be redirected
//! back to the listener with a code, which it trades for tokens. PKCE ties the
//! code to this process, and `state` ties the redirect to this attempt.
//!
//! ## What is stored
//!
//! The refresh token, in the keyring slot a password would otherwise take --
//! the account's TOML says which of the two it is. Access tokens last about an
//! hour and are only ever kept in memory, refreshed on demand.
//!
//! ## Client IDs
//!
//! Every provider wants the client registered with it first. The IDs come from
//! `~/.config/airmail/oauth.toml`, or are baked in at build time through
//! `AIRMAIL_GOOGLE_CLIENT_ID`, `AIRMAIL_GOOGLE_CLIENT_SECRET` and
//! `AIRMAIL_MICROSOFT_CLIENT_ID`. A provider with no client ID configured
//! falls back to an app password, exactly as before this module existed.
//! Google calls its desktop "client secret" a secret, but it cannot be one in
//! a program anyone can download, and Google's docs say as much.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::models::{AccountConfig, OAuthProvider};

/// How long to wait for the browser to come back before giving up.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

/// Refresh this long before the provider says the token expires, so a token
/// does not run out between being handed over and being used.
const EXPIRY_MARGIN: Duration = Duration::from_secs(120);

struct Endpoints {
    authorize: &'static str,
    token: &'static str,
    scope: &'static str,
    /// Where the browser is sent back to, before the port is added. Google
    /// wants the loopback address; Microsoft only accepts `localhost` without
    /// editing the app manifest by hand.
    redirect_host: &'static str,
}

impl OAuthProvider {
    fn endpoints(self) -> Endpoints {
        match self {
            Self::Google => Endpoints {
                authorize: "https://accounts.google.com/o/oauth2/v2/auth",
                token: "https://oauth2.googleapis.com/token",
                scope: "https://mail.google.com/",
                redirect_host: "127.0.0.1",
            },
            // `common` takes personal accounts and work or school ones alike.
            Self::Microsoft => Endpoints {
                authorize: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                token: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                scope: "offline_access https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send",
                redirect_host: "localhost",
            },
        }
    }

    /// What the button and the status line call the provider.
    pub fn label(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Microsoft => "Microsoft",
        }
    }

    /// One-time steps for registering a client with this provider, as Pango
    /// markup for the setup dialog. Shown beside the app-password hint while
    /// no client ID is configured.
    pub fn setup_steps(self) -> &'static str {
        match self {
            Self::Google => {
                "<b>Or sign in with Google in your browser instead.</b> One-time setup: in \
                 <a href=\"https://console.cloud.google.com/apis/credentials\">Google Cloud Console</a>, \
                 enable the Gmail API, set the OAuth consent screen to <i>External</i> and \
                 <i>In production</i>, then create an OAuth client of type <i>Desktop app</i>. \
                 Put its ID and secret in <tt>~/.config/airmail/oauth.toml</tt> under \
                 <tt>[google]</tt> as <tt>client_id</tt> and <tt>client_secret</tt>, then \
                 reopen this dialog."
            }
            Self::Microsoft => {
                "<b>Or sign in with Microsoft in your browser instead.</b> One-time setup: in \
                 <a href=\"https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade\">Azure App registrations</a>, \
                 register an app for personal and work accounts, add the \
                 <i>Mobile and desktop applications</i> platform with redirect URI \
                 <tt>http://localhost</tt>, and turn on <i>Allow public client flows</i>. Put its \
                 Application ID in <tt>~/.config/airmail/oauth.toml</tt> under \
                 <tt>[microsoft]</tt> as <tt>client_id</tt>, then reopen this dialog."
            }
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "microsoft",
        }
    }
}

// ---------------------------------------------------------------- client IDs

#[derive(Debug, Clone, Deserialize)]
struct Client {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
}

fn baked_in(provider: OAuthProvider) -> Option<Client> {
    let (id, secret) = match provider {
        OAuthProvider::Google => (
            option_env!("AIRMAIL_GOOGLE_CLIENT_ID"),
            option_env!("AIRMAIL_GOOGLE_CLIENT_SECRET"),
        ),
        OAuthProvider::Microsoft => (option_env!("AIRMAIL_MICROSOFT_CLIENT_ID"), None),
    };
    Some(Client {
        client_id: id.filter(|id| !id.is_empty())?.to_string(),
        client_secret: secret.map(str::to_string),
    })
}

/// The client registration for a provider: `oauth.toml` first, then whatever
/// the build baked in.
fn client(provider: OAuthProvider) -> Option<Client> {
    let from_file = crate::config::oauth_config_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(
            |text| match toml::from_str::<HashMap<String, Client>>(&text) {
                Ok(table) => Some(table),
                Err(e) => {
                    tracing::warn!("ignoring oauth.toml: {e}");
                    None
                }
            },
        )
        .and_then(|mut table| table.remove(provider.key()))
        .filter(|c| !c.client_id.trim().is_empty());
    from_file.or_else(|| baked_in(provider))
}

/// Whether this machine can sign in to `provider` through the browser.
pub fn is_configured(provider: OAuthProvider) -> bool {
    client(provider).is_some()
}

fn require_client(provider: OAuthProvider) -> Result<Client> {
    client(provider).with_context(|| {
        format!(
            "no {} client ID configured — add a [{}] client_id to {}",
            provider.label(),
            provider.key(),
            crate::config::oauth_config_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "oauth.toml".into()),
        )
    })
}

// ------------------------------------------------------------------- tokens

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Access tokens by account address, with when to stop trusting them.
static ACCESS_TOKENS: LazyLock<Mutex<HashMap<String, (String, Instant)>>> =
    LazyLock::new(Default::default);

fn remember(email: &str, token: &TokenResponse) {
    let lifetime = Duration::from_secs(token.expires_in.unwrap_or(3600));
    let good_until = Instant::now() + lifetime.saturating_sub(EXPIRY_MARGIN);
    if let Ok(mut cache) = ACCESS_TOKENS.lock() {
        cache.insert(email.to_string(), (token.access_token.clone(), good_until));
    }
}

/// Drop a cached access token, so the next use refreshes it. For when the
/// server turned it down before it was due to expire.
pub fn forget(email: &str) {
    if let Ok(mut cache) = ACCESS_TOKENS.lock() {
        cache.remove(email);
    }
}

/// POST a form to a token endpoint. Blocking, so it runs off the executor.
async fn post_token(url: &'static str, form: Vec<(&'static str, String)>) -> Result<TokenResponse> {
    tokio::task::spawn_blocking(move || -> Result<TokenResponse> {
        // Status codes are read by hand: the error body is the useful part.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        let mut response = agent
            .post(url)
            .send_form(form.iter().map(|(k, v)| (*k, v.as_str())))
            .with_context(|| format!("reaching {url}"))?;
        let status = response.status();
        let body = response
            .body_mut()
            .read_to_string()
            .context("reading the token response")?;
        if !status.is_success() {
            return Err(match serde_json::from_str::<TokenError>(&body) {
                Ok(e) => anyhow!(
                    "{} ({})",
                    e.error_description.unwrap_or_else(|| e.error.clone()),
                    e.error
                ),
                Err(_) => anyhow!("token endpoint answered {status}: {body}"),
            });
        }
        serde_json::from_str(&body).context("unexpected token response")
    })
    .await
    .context("token request task")?
}

/// A current access token for an OAuth account, refreshing it if needed.
pub async fn access_token(cfg: &AccountConfig, provider: OAuthProvider) -> Result<String> {
    if let Ok(cache) = ACCESS_TOKENS.lock()
        && let Some((token, good_until)) = cache.get(&cfg.email)
        && Instant::now() < *good_until
    {
        return Ok(token.clone());
    }

    let client = require_client(provider)?;
    let refresh_token = crate::config::get_password(&cfg.email)
        .context("no sign-in stored for this account — remove it and add it again")?;
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.clone()),
        ("client_id", client.client_id),
    ];
    if let Some(secret) = client.client_secret {
        form.push(("client_secret", secret));
    }
    let token = post_token(provider.endpoints().token, form)
        .await
        .with_context(|| {
            format!(
                "refreshing the {} sign-in for {} (remove and re-add the account if this persists)",
                provider.label(),
                cfg.email
            )
        })?;

    // Microsoft rotates refresh tokens; keep the newest one.
    if let Some(new) = &token.refresh_token
        && *new != refresh_token
        && let Err(e) = crate::config::store_password(&cfg.email, new)
    {
        tracing::warn!("could not store the rotated refresh token: {e}");
    }
    remember(&cfg.email, &token);
    Ok(token.access_token)
}

// ---------------------------------------------------------------- sign-in

/// A browser sign-in in progress: the listener is bound and the URL built.
pub struct Pending {
    provider: OAuthProvider,
    email: String,
    client: Client,
    url: String,
    redirect_uri: String,
    state: String,
    verifier: String,
    listeners: Vec<TcpListener>,
}

fn random_token(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| anyhow!("no randomness available: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

impl Pending {
    /// Bind the loopback listener and build the sign-in URL. Nothing leaves
    /// the machine until the URL is opened.
    pub fn start(provider: OAuthProvider, email: &str) -> Result<Self> {
        let client = require_client(provider)?;
        let endpoints = provider.endpoints();

        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .context("opening a local port for the sign-in to come back to")?;
        let port = v4.local_addr()?.port();
        let mut listeners = vec![v4];
        // `localhost` may resolve to ::1 first in the browser. Best effort:
        // the same port may already be taken on the IPv6 side.
        if endpoints.redirect_host == "localhost"
            && let Ok(v6) = TcpListener::bind((Ipv6Addr::LOCALHOST, port))
        {
            listeners.push(v6);
        }
        for listener in &listeners {
            listener.set_nonblocking(true)?;
        }

        let redirect_uri = format!("http://{}:{port}", endpoints.redirect_host);
        let state = random_token(16)?;
        let verifier = random_token(48)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

        let mut url = url::Url::parse(endpoints.authorize)?;
        url.query_pairs_mut()
            .append_pair("client_id", &client.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", endpoints.scope)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("login_hint", email.trim());
        if provider == OAuthProvider::Google {
            // Without both, Google hands out a refresh token only the first
            // time an account ever grants access.
            url.query_pairs_mut()
                .append_pair("access_type", "offline")
                .append_pair("prompt", "consent");
        }

        Ok(Self {
            provider,
            email: email.trim().to_string(),
            client,
            url: url.into(),
            redirect_uri,
            state,
            verifier,
            listeners,
        })
    }

    /// The page to open in the browser.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Wait for the browser to come back, then trade the code for tokens.
    /// Returns the refresh token, which is what goes in the keyring. Must run
    /// on a tokio runtime.
    pub async fn finish(self) -> Result<String> {
        let code = tokio::time::timeout(SIGN_IN_TIMEOUT, self.wait_for_code())
            .await
            .map_err(|_| anyhow!("gave up waiting for the browser sign-in after 5 minutes"))??;

        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code),
            ("redirect_uri", self.redirect_uri.clone()),
            ("client_id", self.client.client_id.clone()),
            ("code_verifier", self.verifier.clone()),
        ];
        if let Some(secret) = &self.client.client_secret {
            form.push(("client_secret", secret.clone()));
        }
        let token = post_token(self.provider.endpoints().token, form)
            .await
            .context("finishing the sign-in")?;
        let refresh = token.refresh_token.clone().with_context(|| {
            format!(
                "{} did not grant offline access, so AirMail could not stay signed in",
                self.provider.label()
            )
        })?;
        remember(&self.email, &token);
        Ok(refresh)
    }

    async fn wait_for_code(&self) -> Result<String> {
        let listeners = self
            .listeners
            .iter()
            .map(|l| {
                l.try_clone()
                    .and_then(tokio::net::TcpListener::from_std)
                    .context("watching the local sign-in port")
            })
            .collect::<Result<Vec<_>>>()?;

        loop {
            let accepts = listeners.iter().map(|l| Box::pin(l.accept()));
            let (accepted, _, _) = futures::future::select_all(accepts).await;
            let (stream, _) = accepted.context("accepting the browser's redirect")?;
            let mut stream = stream.into_std()?;
            stream.set_nonblocking(false)?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;

            let Some(target) = read_request_target(&mut stream) else {
                continue;
            };
            let url = url::Url::parse(&format!("http://localhost{target}"))?;
            let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

            // Favicons and other stray requests: not the redirect.
            if !params.contains_key("code") && !params.contains_key("error") {
                let _ = respond(&mut stream, "404 Not Found", "");
                continue;
            }
            if params.get("state") != Some(&self.state) {
                let _ = respond(&mut stream, "400 Bad Request", PAGE_FAILED);
                bail!("the sign-in response did not match this attempt; try again");
            }
            if let Some(error) = params.get("error") {
                let _ = respond(&mut stream, "200 OK", PAGE_FAILED);
                let detail = params.get("error_description").unwrap_or(error);
                bail!(
                    "{} sign-in was not completed: {detail}",
                    self.provider.label()
                );
            }
            let _ = respond(&mut stream, "200 OK", PAGE_DONE);
            return Ok(params["code"].clone());
        }
    }
}

/// The request target of an HTTP request line, e.g. `/?code=…&state=…`.
fn read_request_target(stream: &mut std::net::TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let mut parts = text.lines().next()?.split_whitespace();
    (parts.next()? == "GET").then_some(())?;
    parts.next().map(str::to_string)
}

fn respond(stream: &mut std::net::TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

const PAGE_DONE: &str = "<!doctype html><meta charset=utf-8><title>AirMail</title>\
<body style=\"font:16px system-ui;text-align:center;padding-top:20vh\">\
<h2>Done here</h2><p>You can close this tab and go back to AirMail.</p>";

const PAGE_FAILED: &str = "<!doctype html><meta charset=utf-8><title>AirMail</title>\
<body style=\"font:16px system-ui;text-align:center;padding-top:20vh\">\
<h2>Sign-in didn't finish</h2><p>Go back to AirMail and try again.</p>";

// ------------------------------------------------------------------ logins

/// What to authenticate IMAP and SMTP with.
pub enum Login {
    Password(String),
    /// An OAuth access token, sent with SASL XOAUTH2.
    Bearer(String),
}

/// The credential for an account, whichever kind it uses.
pub async fn login_for(cfg: &AccountConfig) -> Result<Login> {
    match cfg.oauth {
        Some(provider) => Ok(Login::Bearer(access_token(cfg, provider).await?)),
        None => Ok(Login::Password(crate::config::get_password(&cfg.email)?)),
    }
}

/// The XOAUTH2 initial response for `user`.
pub fn xoauth2_payload(user: &str, token: &str) -> String {
    format!("user={user}\x01auth=Bearer {token}\x01\x01")
}
