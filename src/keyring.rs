//! Account passwords, kept in HuginnKeyring.
//!
//! One connection per operation, over the native socket at
//! `$XDG_RUNTIME_DIR/huginn-keyring/socket`. The socket is local and the
//! daemon answers in microseconds, so these three functions block rather than
//! going async: every caller is either the GUI thread between a click and a
//! redraw, or `--doctor` at a terminal, and neither is doing anything else
//! while it waits.
//!
//! ## How an account is filed
//!
//! Two attributes, `service` and `username`, which is the pair libsecret's
//! password API writes and the pair `secret-tool` expects. Items are matched
//! on a *subset* of their attributes, so those two also find what the
//! `keyring` crate stored before this module existed -- it wrote the same two
//! names plus a `target` of its own. Nobody has to retype a password over this
//! change, and [`store`] clears the older duplicate the first time it writes.
//!
//! ## What is not here
//!
//! Unlocking. If the login keyring is locked these functions say so and stop,
//! because the only way to ask for a password is the daemon's prompter socket
//! and nothing draws that dialog yet. Guessing at one inside a mail client
//! would be a second place for a keyring password to be typed, which is one
//! more than there should be.

use anyhow::{Context, Result, anyhow, bail};
use huginn_wire::attributes::SCHEMA_ATTRIBUTE;
use huginn_wire::proto::{ErrorCode, ItemInfo, Request, Response};
use huginn_wire::{Attributes, Client, SecretBytes};

/// The collection to write to: whatever `default` points at, which on a Raven
/// desktop is the login keyring PAM opened during login.
const COLLECTION: &str = "default";

/// The `service` attribute every item of ours carries.
const SERVICE: &str = "dev.airmail";

/// libsecret's schema marker. Nothing here reads it back; it is written so
/// that `secret-tool` and the keyring browsers show these as the ordinary
/// stored passwords they are, rather than as items of an unknown kind.
const SCHEMA: &str = "org.freedesktop.Secret.Generic";

/// What one account's password is filed under.
fn query(email: &str) -> Attributes {
    let mut attributes = Attributes::new();
    attributes.insert("service", SERVICE);
    attributes.insert("username", email);
    attributes
}

/// Connect to this session's daemon.
fn connect() -> Result<Client> {
    let path = Client::socket_path()
        .context("there is no session keyring to talk to (XDG_RUNTIME_DIR is unset)")?;
    Client::connect_to(&path).with_context(|| {
        format!(
            "cannot reach the keyring at {} — is huginn-keyringd running?",
            path.display()
        )
    })
}

/// Send a request and turn a refusal into an error.
fn require(client: &mut Client, request: &Request) -> Result<Response> {
    match client.call(request)? {
        Response::Error { code, message } => Err(anyhow!("{}", explain(code, &message))),
        other => Ok(other),
    }
}

/// The daemon's messages are accurate and terse. These add the part a person
/// in front of a mail client needs, which is what to do next.
fn explain(code: ErrorCode, message: &str) -> String {
    match code {
        ErrorCode::Locked => format!("{message} — unlock it with `huginn-keyring unlock`"),
        ErrorCode::NoSuchCollection => {
            format!("{message} — `huginn-keyring create` makes one")
        }
        ErrorCode::Dismissed => "the keyring prompt was dismissed".to_owned(),
        _ => message.to_owned(),
    }
}

/// Every item filed under `email`, in one collection or in all of them.
fn search(client: &mut Client, collection: Option<&str>, email: &str) -> Result<Vec<ItemInfo>> {
    let response = require(
        client,
        &Request::Search {
            collection: collection.map(str::to_owned),
            attributes: query(email),
        },
    )?;
    match response {
        Response::Items(items) => Ok(items),
        other => bail!("the keyring answered a search with {other:?}"),
    }
}

/// Save an account's password, replacing any password already stored for it.
pub fn store(email: &str, password: &str) -> Result<()> {
    let mut client = connect()?;

    let mut attributes = query(email);
    attributes.insert(SCHEMA_ATTRIBUTE, SCHEMA);

    let response = require(
        &mut client,
        &Request::Store {
            collection: COLLECTION.to_owned(),
            label: format!("AirMail — {email}"),
            attributes,
            secret: SecretBytes::from(password.as_bytes()),
            content_type: "text/plain".to_owned(),
            replace: true,
        },
    )?;
    let Response::Id(stored) = response else {
        bail!("the keyring answered a store with {response:?}");
    };

    // `replace` replaces the item whose attributes *equal* the ones just
    // written, so an older item carrying an extra attribute -- which is every
    // item the `keyring` crate wrote, with its `target` -- survives the store
    // and would then race it on the next read. Drop those, and do it after the
    // new password is safely down rather than before.
    for stale in search(&mut client, Some(COLLECTION), email)? {
        if stale.id == stored {
            continue;
        }
        require(
            &mut client,
            &Request::DeleteItem {
                collection: stale.collection,
                item: stale.id,
            },
        )?;
    }

    Ok(())
}

/// Read an account's password back.
pub fn get(email: &str) -> Result<String> {
    let mut client = connect()?;
    let items = search(&mut client, None, email)?;

    // A locked collection still reports its matches -- with no label and no
    // secret -- which is the difference between "you never saved this" and
    // "you saved it and the keyring is shut", and those deserve different
    // sentences.
    let Some(item) = items.iter().find(|i| !i.locked) else {
        if items.iter().any(|i| i.locked) {
            bail!(
                "the password for {email} is in a locked keyring \
                 — unlock it with `huginn-keyring unlock`"
            );
        }
        bail!("no password stored for {email}");
    };

    let response = require(
        &mut client,
        &Request::GetSecret {
            collection: item.collection.clone(),
            item: item.id.clone(),
        },
    )?;
    let Response::Secret { secret, .. } = response else {
        bail!("the keyring answered a secret request with {response:?}");
    };

    // This is where the password stops being wiped-on-drop, because IMAP and
    // SMTP authentication take a `&str`. The copy is deliberate and it is the
    // narrowest one available; shortening its life means changing what
    // `async-imap` and `lettre` are handed, not what is done here.
    String::from_utf8(secret.expose().to_vec())
        .with_context(|| format!("the stored password for {email} is not text"))
}

/// Forget an account's password, wherever it is filed.
pub fn delete(email: &str) -> Result<()> {
    let mut client = connect()?;
    for item in search(&mut client, None, email)? {
        require(
            &mut client,
            &Request::DeleteItem {
                collection: item.collection,
                item: item.id,
            },
        )?;
    }
    Ok(())
}
