use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config;
use crate::db::Db;
use crate::mailparse;
use crate::models::AccountConfig;

/// Events pushed from sync workers to the UI thread.
#[derive(Debug)]
pub enum SyncEvent {
    /// Incremental refresh signal; UI re-queries the database.
    Updated { account: String, folder: String },
    NewMessages { account: String, count: usize },
    Error { account: String, message: String },
}

const POLL_INTERVAL: Duration = Duration::from_secs(30);
const MAX_MESSAGES_ON_FIRST_SYNC: usize = 200;

/// Spawn one sync loop per account on the current tokio runtime.
/// Returns the event receiver plus handles for aborting on reconfigure.
pub fn start_sync(
    db_path: PathBuf,
    accounts: Arc<Vec<AccountConfig>>,
) -> (mpsc::UnboundedReceiver<SyncEvent>, Vec<JoinHandle<()>>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let handles = accounts
        .iter()
        .map(|account| {
            let account = account.clone();
            let tx = tx.clone();
            let db_path = db_path.clone();
            tokio::spawn(async move {
                loop {
                    if let Err(e) = sync_once(&db_path, &account, &tx).await {
                        let _ = tx.send(SyncEvent::Error {
                            account: account.email.clone(),
                            message: format!("{e:#}"),
                        });
                    }
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            })
        })
        .collect();
    (rx, handles)
}

async fn sync_once(
    db_path: &std::path::Path,
    account: &AccountConfig,
    tx: &mpsc::UnboundedSender<SyncEvent>,
) -> Result<()> {
    let password = config::get_password(&account.email)?;
    let mut session = crate::sync::imap::connect(account, &password).await?;

    // Opening the DB connection can block on locks; do it off the executor.
    let mut db = tokio::task::spawn_blocking({
        let db_path = db_path.to_path_buf();
        move || Db::open(&db_path)
    })
    .await
    .context("opening database")??;
    let account_db_id = db.upsert_account(account)?;

    let folders = crate::sync::imap::list_folders(&mut session).await?;
    db.delete_missing_folders(account_db_id, &folders)?;

    // Remember the sent folder once: first folder whose name mentions "sent".
    if db.list_accounts()?.iter().find(|a| a.id == account_db_id).and_then(|a| a.sent_folder.clone()).is_none()
        && let Some(sent) = folders.iter().find(|f| f.to_lowercase().contains("sent"))
    {
        db.set_sent_folder(account_db_id, sent)?;
    }

    let mut new_total = 0usize;
    for folder_name in &folders {
        let n = sync_folder(
            &mut db,
            &mut session,
            account_db_id,
            folder_name,
            tx,
            &account.email,
        )
        .await
        .with_context(|| format!("syncing folder {folder_name:?}"))?;
        new_total += n;
    }

    if new_total > 0 {
        let _ = tx.send(SyncEvent::NewMessages {
            account: account.email.clone(),
            count: new_total,
        });
    }

    let _ = session.logout().await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn sync_folder(
    db: &mut Db,
    session: &mut crate::sync::imap::ImapSession,
    account_db_id: i64,
    folder_name: &str,
    tx: &mpsc::UnboundedSender<SyncEvent>,
    account_email: &str,
) -> Result<usize> {
    let info = session.examine(folder_name).await?;
    let folder_db_id = db.upsert_folder(account_db_id, folder_name, info.uid_validity)?;
    let folder = db
        .folder(folder_db_id)?
        .context("folder vanished from database")?;

    let uid_set = if folder.last_seen_uid == 0 {
        // First sync of this folder: grab the most recent N messages only.
        let uids = crate::sync::imap::all_uids(session, folder_name).await?;
        let recent: Vec<u32> = uids
            .iter()
            .rev()
            .take(MAX_MESSAGES_ON_FIRST_SYNC)
            .rev()
            .copied()
            .collect();
        if recent.is_empty() {
            String::new()
        } else if recent.len() == 1 {
            recent[0].to_string()
        } else {
            format!("{}:{}", recent[0], recent[recent.len() - 1])
        }
    } else {
        // "n:*" always includes at least the highest UID in the mailbox;
        // entries at or below the watermark are skipped below.
        format!("{}:*", folder.last_seen_uid + 1)
    };
    if uid_set.is_empty() {
        return Ok(0);
    }

    let fetched = crate::sync::imap::fetch_messages(session, folder_name, &uid_set).await?;

    let mut max_uid = folder.last_seen_uid;
    let mut stored = 0usize;
    for msg in fetched {
        if msg.uid <= folder.last_seen_uid {
            continue; // overlap from the "n:*" range
        }
        let parsed = mailparse::parse(&msg.raw).unwrap_or_default();
        db.store_message(
            folder_db_id,
            msg.uid,
            &parsed.subject,
            &parsed.from,
            &parsed.to,
            parsed.date,
            msg.seen,
            parsed.has_attachments,
            &parsed.body_text,
            &parsed.body_html,
            &msg.raw,
        )?;
        max_uid = max_uid.max(msg.uid);
        stored += 1;
    }
    if max_uid > folder.last_seen_uid {
        db.set_last_seen_uid(folder_db_id, max_uid)?;
    }
    if stored > 0 {
        let _ = tx.send(SyncEvent::Updated {
            account: account_email.to_string(),
            folder: folder_name.to_string(),
        });
    }
    Ok(stored)
}
