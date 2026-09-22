//! Account passwords, kept in HuginnKeyring.
//!
//! One connection per operation, over the native socket at
//! `$XDG_RUNTIME_DIR/huginn-keyring/socket`. The socket is local and the
//! daemon answers in microseconds, so these functions block rather than going
//! async: every caller is either the GUI thread between a click and a redraw,
//! or `--doctor` at a terminal, and neither is doing anything else while it
//! waits.
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
//! ## When there is no keyring at all
//!
//! A machine where nothing has ever created one -- no login handoff, no
//! `huginn-keyring create` -- has nowhere to put a password, and that is the
//! one failure a person in front of a mail client can fix. It gets a variant
//! of its own, [`Error::NoKeyring`], so the UI can offer [`create`] instead of
//! printing a sentence with a shell command in it. Everything else is
//! [`Error::Other`]: a locked keyring, a daemon that is not running, a socket
//! that is not there.

use anyhow::{Context, Result, anyhow};
use huginn_wire::attributes::SCHEMA_ATTRIBUTE;
use huginn_wire::proto::{ErrorCode, ItemInfo, Request, Response};
use huginn_wire::{Attributes, Client, SecretBytes};

/// The collection to write to: whatever `default` points at, which on a Raven
/// desktop is the login keyring the handoff opened during login.
const COLLECTION: &str = "default";

/// What [`create`] calls the keyring it makes.
///
/// Not decorative. The daemon derives a collection's identifier by slugifying
/// its label, so this one becomes `login` -- which is the exact identifier the
/// login handoff looks for. A keyring made here under the user's login
/// password is therefore the same keyring `ravend` would have made, and the
/// first login after the handoff starts working opens it rather than making a
/// second one beside it. See the dialog in `ui::keyring_setup` for the other
/// half of that promise, which is telling the user which password to type.
const LABEL: &str = "Login";

/// The `service` attribute every item of ours carries.
const SERVICE: &str = "dev.airmail";

/// libsecret's schema marker. Nothing here reads it back; it is written so
/// that `secret-tool` and the keyring browsers show these as the ordinary
/// stored passwords they are, rather than as items of an unknown kind.
const SCHEMA: &str = "org.freedesktop.Secret.Generic";

/// What went wrong reaching the keyring.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// There is no keyring on this computer at all. The only failure the user
    /// can do something about from inside AirMail, which is why it is the only
    /// one with a variant to itself.
    #[error("there is no keyring on this computer yet")]
    NoKeyring,
    /// Everything else, already carrying whatever the daemon said and what to
    /// do about it.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

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

/// Send a request, and turn a refusal into an [`Error`].
fn call(client: &mut Client, request: &Request) -> Result<Response, Error> {
    match client.call(request).map_err(anyhow::Error::from)? {
        Response::Error { code, message } => Err(refusal(client, code, &message)),
        other => Ok(other),
    }
}

/// Work out which [`Error`] a refusal is.
///
/// The daemon's messages are accurate and terse. These add the part a person
/// in front of a mail client needs, which is what to do next.
fn refusal(client: &mut Client, code: ErrorCode, message: &str) -> Error {
    match code {
        // `default` resolved to nothing, which is two different situations.
        // Either there are no keyrings at all and we can offer to make one, or
        // there are some and none of them is the default -- which AirMail must
        // not fix by guessing, because picking somebody's default keyring for
        // them is not a mail client's decision.
        ErrorCode::NoSuchCollection => match count_collections(client) {
            Ok(0) => Error::NoKeyring,
            Ok(_) => Error::Other(anyhow!(
                "{message} — no keyring is the default one; \
                 point `default` at one with `huginn-keyring alias default <keyring>`"
            )),
            // Asking made it worse. Report what was actually refused.
            Err(e) => Error::Other(e.context(message.to_owned())),
        },
        ErrorCode::Locked => Error::Other(anyhow!(
            "{message} — unlock it with `huginn-keyring unlock`"
        )),
        ErrorCode::Dismissed => Error::Other(anyhow!("the keyring prompt was dismissed")),
        _ => Error::Other(anyhow!("{message}")),
    }
}

/// How many keyrings exist, default or not.
///
/// Deliberately not routed through [`call`], which would ask this question
/// again about the answer to this question.
fn count_collections(client: &mut Client) -> Result<usize> {
    match client.call(&Request::ListCollections)? {
        Response::Collections(collections) => Ok(collections.len()),
        Response::Error { message, .. } => Err(anyhow!("{message}")),
        other => Err(anyhow!("the keyring answered a list with {other:?}")),
    }
}

/// Every item filed under `email`, in one collection or in all of them.
fn search(
    client: &mut Client,
    collection: Option<&str>,
    email: &str,
) -> Result<Vec<ItemInfo>, Error> {
    let response = call(
        client,
        &Request::Search {
            collection: collection.map(str::to_owned),
            attributes: query(email),
        },
    )?;
    match response {
        Response::Items(items) => Ok(items),
        other => Err(anyhow!("the keyring answered a search with {other:?}").into()),
    }
}

/// Make the keyring everything else here needs, sealed under `password` and
/// pointed at by `default`.
///
/// For the one case the user can fix from inside AirMail. See [`LABEL`] for
/// why the name is not arbitrary.
pub fn create(password: &str) -> Result<(), Error> {
    let mut client = connect()?;
    let response = call(
        &mut client,
        &Request::CreateCollection {
            label: LABEL.to_owned(),
            alias: Some(COLLECTION.to_owned()),
            password: Some(SecretBytes::from(password.as_bytes())),
        },
    )?;
    match response {
        Response::Id(_) => Ok(()),
        other => Err(anyhow!("the keyring answered a create with {other:?}").into()),
    }
}

/// Save an account's password, replacing any password already stored for it.
pub fn store(email: &str, password: &str) -> Result<(), Error> {
    let mut client = connect()?;

    let mut attributes = query(email);
    attributes.insert(SCHEMA_ATTRIBUTE, SCHEMA);

    let response = call(
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
        return Err(anyhow!("the keyring answered a store with {response:?}").into());
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
        call(
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
pub fn get(email: &str) -> Result<String, Error> {
    let mut client = connect()?;
    let items = search(&mut client, None, email)?;

    // A locked collection still reports its matches -- with no label and no
    // secret -- which is the difference between "you never saved this" and
    // "you saved it and the keyring is shut", and those deserve different
    // sentences.
    let Some(item) = items.iter().find(|i| !i.locked) else {
        return Err(if items.iter().any(|i| i.locked) {
            anyhow!(
                "the password for {email} is in a locked keyring \
                 — unlock it with `huginn-keyring unlock`"
            )
        } else {
            anyhow!("no password stored for {email}")
        }
        .into());
    };

    let response = call(
        &mut client,
        &Request::GetSecret {
            collection: item.collection.clone(),
            item: item.id.clone(),
        },
    )?;
    let Response::Secret { secret, .. } = response else {
        return Err(anyhow!("the keyring answered a secret request with {response:?}").into());
    };

    // This is where the password stops being wiped-on-drop, because IMAP and
    // SMTP authentication take a `&str`. The copy is deliberate and it is the
    // narrowest one available; shortening its life means changing what
    // `async-imap` and `lettre` are handed, not what is done here.
    String::from_utf8(secret.expose().to_vec())
        .with_context(|| format!("the stored password for {email} is not text"))
        .map_err(Error::Other)
}

/// Forget an account's password, wherever it is filed.
pub fn delete(email: &str) -> Result<(), Error> {
    let mut client = connect()?;
    for item in search(&mut client, None, email)? {
        call(
            &mut client,
            &Request::DeleteItem {
                collection: item.collection,
                item: item.id,
            },
        )?;
    }
    Ok(())
}
