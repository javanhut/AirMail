use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};

use crate::models::{Account, AccountConfig, Folder, MessageDetail, MessageSummary, SmtpSecurity};

const SCHEMA_VERSION: i64 = 3;

/// One connection per thread/process segment. The GUI thread and each sync
/// worker open their own connection; SQLite WAL mode coordinates them.
pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS accounts (
                    id            INTEGER PRIMARY KEY,
                    email         TEXT UNIQUE NOT NULL,
                    display_name  TEXT,
                    username      TEXT,
                    imap_host     TEXT NOT NULL,
                    imap_port     INTEGER NOT NULL,
                    smtp_host     TEXT NOT NULL,
                    smtp_port     INTEGER NOT NULL,
                    sent_folder   TEXT
                );

                CREATE TABLE IF NOT EXISTS folders (
                    id             INTEGER PRIMARY KEY,
                    account_id     INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                    name           TEXT NOT NULL,
                    uid_validity   INTEGER NOT NULL DEFAULT 0,
                    last_seen_uid  INTEGER NOT NULL DEFAULT 0,
                    UNIQUE(account_id, name)
                );

                CREATE TABLE IF NOT EXISTS messages (
                    id               INTEGER PRIMARY KEY,
                    folder_id        INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
                    uid              INTEGER NOT NULL,
                    message_id       TEXT,
                    subject          TEXT NOT NULL DEFAULT '',
                    sender           TEXT NOT NULL DEFAULT '',
                    recipients       TEXT NOT NULL DEFAULT '',
                    date             INTEGER,
                    seen             INTEGER NOT NULL DEFAULT 0,
                    has_attachments  INTEGER NOT NULL DEFAULT 0,
                    body_text        TEXT NOT NULL DEFAULT '',
                    body_html        TEXT NOT NULL DEFAULT '',
                    raw              BLOB,
                    UNIQUE(folder_id, uid)
                );

                CREATE INDEX IF NOT EXISTS idx_messages_folder ON messages(folder_id, uid);
                CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(date);
                ",
            )?;
        }
        if version < 2 {
            // Added when Outlook and iCloud turned out to need STARTTLS rather
            // than the implicit TLS every account had been assumed to use.
            self.conn.execute_batch(
                "ALTER TABLE accounts ADD COLUMN smtp_security TEXT NOT NULL DEFAULT 'tls';",
            )?;
        }
        if version < 3 {
            // The star. Existing rows default to unflagged and pick the real
            // value up on the next sync, which re-fetches FLAGS anyway.
            self.conn.execute_batch(
                "ALTER TABLE messages ADD COLUMN flagged INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    pub fn upsert_account(&self, cfg: &AccountConfig) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO accounts (email, display_name, username, imap_host, imap_port, smtp_host, smtp_port, smtp_security)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(email) DO UPDATE SET
               display_name  = excluded.display_name,
               username      = excluded.username,
               imap_host     = excluded.imap_host,
               imap_port     = excluded.imap_port,
               smtp_host     = excluded.smtp_host,
               smtp_port     = excluded.smtp_port,
               smtp_security = excluded.smtp_security",
            params![
                cfg.email,
                cfg.display_name,
                cfg.username,
                cfg.imap_host,
                cfg.imap_port,
                cfg.smtp_host,
                cfg.smtp_port,
                smtp_security_text(cfg.smtp_security),
            ],
        )?;
        Ok(self.conn.query_row(
            "SELECT id FROM accounts WHERE email = ?1",
            params![cfg.email],
            |row| row.get(0),
        )?)
    }

    pub fn remove_account(&self, email: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM accounts WHERE email = ?1", params![email])?;
        Ok(())
    }

    pub fn set_sent_folder(&self, account_db_id: i64, folder: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE accounts SET sent_folder = ?2 WHERE id = ?1",
            params![account_db_id, folder],
        )?;
        Ok(())
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, email, display_name, username, imap_host, imap_port, smtp_host, smtp_port, sent_folder, smtp_security
             FROM accounts ORDER BY email",
        )?;
        let rows = stmt.query_map([], |row| {
            let email: String = row.get(1)?;
            let config = AccountConfig {
                email,
                display_name: row.get(2)?,
                username: row.get(3)?,
                imap_host: row.get(4)?,
                imap_port: row.get(5)?,
                smtp_host: row.get(6)?,
                smtp_port: row.get(7)?,
                smtp_security: smtp_security_from(&row.get::<_, String>(9)?),
                // How it signs in lives in the account's TOML, not here.
                oauth: None,
            };
            Ok(Account {
                id: row.get(0)?,
                config,
                sent_folder: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Returns the folder id. If the server-reported `uid_validity` differs from
    /// what we have, all cached messages for the folder are purged and syncing
    /// restarts from scratch (per RFC 3501 UID semantics).
    pub fn upsert_folder(
        &self,
        account_db_id: i64,
        name: &str,
        uid_validity: Option<u32>,
    ) -> Result<i64> {
        let uid_validity = i64::from(uid_validity.unwrap_or(0));
        let existing: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT id, uid_validity FROM folders WHERE account_id = ?1 AND name = ?2",
                params![account_db_id, name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        match existing {
            Some((id, known_validity)) if known_validity == uid_validity => Ok(id),
            Some((id, _)) => {
                self.conn
                    .execute("DELETE FROM messages WHERE folder_id = ?1", params![id])?;
                self.conn.execute(
                    "UPDATE folders SET uid_validity = ?3, last_seen_uid = 0 WHERE id = ?1 AND account_id = ?2",
                    params![id, account_db_id, uid_validity],
                )?;
                Ok(id)
            }
            _ => {
                self.conn.execute(
                    "INSERT INTO folders (account_id, name, uid_validity) VALUES (?1, ?2, ?3)
                     ON CONFLICT(account_id, name) DO UPDATE SET uid_validity = excluded.uid_validity",
                    params![account_db_id, name, uid_validity],
                )?;
                Ok(self.conn.query_row(
                    "SELECT id FROM folders WHERE account_id = ?1 AND name = ?2",
                    params![account_db_id, name],
                    |row| row.get(0),
                )?)
            }
        }
    }

    pub fn delete_missing_folders(
        &self,
        account_db_id: i64,
        present_names: &[String],
    ) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM folders WHERE account_id = ?1")?;
        let rows = stmt
            .query_map(params![account_db_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, name) in rows {
            if !present_names.contains(&name) {
                self.conn
                    .execute("DELETE FROM folders WHERE id = ?1", params![id])?;
            }
        }
        Ok(())
    }

    pub fn folders(&self, account_db_id: i64) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, uid_validity, last_seen_uid FROM folders
             WHERE account_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map(params![account_db_id], |row| {
            Ok(Folder {
                id: row.get(0)?,
                account_id: account_db_id,
                name: row.get(1)?,
                uid_validity: row.get(2)?,
                last_seen_uid: row.get::<_, i64>(3)?.clamp(0, u32::MAX as i64) as u32,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn folder(&self, folder_db_id: i64) -> Result<Option<Folder>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, name, uid_validity, last_seen_uid FROM folders WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![folder_db_id], |row| {
            Ok(Folder {
                id: folder_db_id,
                account_id: row.get(0)?,
                name: row.get(1)?,
                uid_validity: row.get(2)?,
                last_seen_uid: row.get::<_, i64>(3)?.clamp(0, u32::MAX as i64) as u32,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_last_seen_uid(&self, folder_db_id: i64, uid: u32) -> Result<()> {
        self.conn.execute(
            "UPDATE folders SET last_seen_uid = ?2 WHERE id = ?1",
            params![folder_db_id, i64::from(uid)],
        )?;
        Ok(())
    }

    /// Cap for storing raw RFC822 payloads; bodies are still stored parsed above this.
    const RAW_CAP: usize = 25 * 1024 * 1024;

    #[allow(clippy::too_many_arguments)]
    pub fn store_message(
        &self,
        folder_db_id: i64,
        uid: u32,
        subject: &str,
        from: &str,
        to: &str,
        date: Option<DateTime<Utc>>,
        seen: bool,
        flagged: bool,
        has_attachments: bool,
        body_text: &str,
        body_html: &str,
        raw: &[u8],
    ) -> Result<i64> {
        let date = date.map(|d| d.timestamp());
        let raw: Option<&[u8]> = if raw.len() <= Self::RAW_CAP {
            Some(raw)
        } else {
            None
        };
        self.conn.execute(
            "INSERT INTO messages
               (folder_id, uid, subject, sender, recipients, date, seen, flagged, has_attachments, body_text, body_html, raw)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(folder_id, uid) DO UPDATE SET
               subject = excluded.subject,
               sender = excluded.sender,
               recipients = excluded.recipients,
               date = excluded.date,
               seen = excluded.seen,
               flagged = excluded.flagged,
               has_attachments = excluded.has_attachments,
               body_text = excluded.body_text,
               body_html = excluded.body_html,
               raw = excluded.raw",
            params![
                folder_db_id,
                i64::from(uid),
                subject,
                from,
                to,
                date,
                seen as i64,
                flagged as i64,
                has_attachments as i64,
                body_text,
                body_html,
                raw,
            ],
        )?;
        Ok(self.conn.query_row(
            "SELECT id FROM messages WHERE folder_id = ?1 AND uid = ?2",
            params![folder_db_id, i64::from(uid)],
            |row| row.get(0),
        )?)
    }

    /// Message list for a scope of folders: `None` means every folder the
    /// database knows about, which is what the smart views over all accounts
    /// want; a slice restricts it to those folder ids.
    ///
    /// The preview column is squashed to a single line here rather than in the
    /// row builder: SQLite can do it once per row, and the label never has to
    /// strip newlines out of a body that may be tens of kilobytes long.
    pub fn message_summaries(
        &self,
        folder_db_ids: Option<&[i64]>,
        limit: usize,
    ) -> Result<Vec<MessageSummary>> {
        let scope = match folder_db_ids {
            None => String::new(),
            Some([]) => return Ok(Vec::new()),
            Some(ids) => format!(
                " AND m.folder_id IN ({})",
                ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
            ),
        };
        let sql = format!(
            "SELECT m.id, a.email, f.name, m.uid, m.subject, m.sender, m.date, m.seen,
                    m.flagged, m.has_attachments,
                    replace(replace(substr(m.body_text, 1, 240), char(13), ' '), char(10), ' ')
             FROM messages m
             JOIN folders f ON f.id = m.folder_id
             JOIN accounts a ON a.id = f.account_id
             WHERE 1 = 1{scope}
             ORDER BY m.date DESC NULLS LAST, m.id DESC
             LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit as i64], |row| summary_from_row(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Everything on file from one correspondent, newest first — the list the
    /// contact pane shows beside an open message. Matched on the address so a
    /// sender who changes their display name still lines up.
    pub fn messages_from(&self, address: &str, limit: usize) -> Result<Vec<MessageSummary>> {
        if address.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT m.id, a.email, f.name, m.uid, m.subject, m.sender, m.date, m.seen,
                    m.flagged, m.has_attachments,
                    replace(replace(substr(m.body_text, 1, 240), char(13), ' '), char(10), ' ')
             FROM messages m
             JOIN folders f ON f.id = m.folder_id
             JOIN accounts a ON a.id = f.account_id
             WHERE lower(m.sender) LIKE '%' || ?1 || '%'
             ORDER BY m.date DESC NULLS LAST, m.id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![address.trim().to_lowercase(), limit as i64],
            |row| summary_from_row(row, 0),
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// How much mail one correspondent has sent, and when it started — the
    /// "About" facts in the contact pane.
    pub fn sender_stats(&self, address: &str) -> Result<SenderStats> {
        if address.trim().is_empty() {
            return Ok(SenderStats::default());
        }
        Ok(self.conn.query_row(
            "SELECT COUNT(*), MIN(m.date), MAX(m.date)
             FROM messages m WHERE lower(m.sender) LIKE '%' || ?1 || '%'",
            params![address.trim().to_lowercase()],
            |row| {
                let first: Option<i64> = row.get(1)?;
                let last: Option<i64> = row.get(2)?;
                Ok(SenderStats {
                    messages: row.get(0)?,
                    first: first.and_then(|t| DateTime::<Utc>::from_timestamp(t, 0)),
                    last: last.and_then(|t| DateTime::<Utc>::from_timestamp(t, 0)),
                })
            },
        )?)
    }

    /// Message ids that arrived today, in local time. The sidebar's "Today"
    /// row counts these and the view itself selects on them.
    pub fn today_ids(&self) -> Result<Vec<i64>> {
        let start = local_day_start();
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM messages WHERE date >= ?1")?;
        let rows = stmt.query_map(params![start], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Unread messages that arrived today, for the "Today" badge.
    pub fn unread_today(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE seen = 0 AND date >= ?1",
            params![local_day_start()],
            |row| row.get(0),
        )?)
    }

    pub fn count_flagged(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE flagged = 1",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn message_detail(&self, message_db_id: i64) -> Result<Option<MessageDetail>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, a.email, f.name, m.uid, m.subject, m.sender, m.date, m.seen,
                    m.flagged, m.has_attachments,
                    replace(replace(substr(m.body_text, 1, 240), char(13), ' '), char(10), ' '),
                    m.recipients, m.body_text, m.body_html
             FROM messages m
             JOIN folders f ON f.id = m.folder_id
             JOIN accounts a ON a.id = f.account_id
             WHERE m.id = ?1",
        )?;
        let mut rows = stmt.query_map(params![message_db_id], |row| {
            Ok(MessageDetail {
                summary: summary_from_row(row, 0)?,
                to: row.get(11)?,
                body_text: row.get(12)?,
                body_html: row.get(13)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_seen(&self, message_db_id: i64, seen: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET seen = ?2 WHERE id = ?1",
            params![message_db_id, seen as i64],
        )?;
        Ok(())
    }

    pub fn set_flagged(&self, message_db_id: i64, flagged: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET flagged = ?2 WHERE id = ?1",
            params![message_db_id, flagged as i64],
        )?;
        Ok(())
    }

    /// Unread message count per folder id, for the badges in the sidebar.
    /// Folders with nothing unread are left out.
    pub fn unread_counts(&self) -> Result<HashMap<i64, i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT folder_id, COUNT(*) FROM messages WHERE seen = 0 GROUP BY folder_id",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Total message count per folder id, for the badges that count messages
    /// rather than unread ones — Drafts, mostly.
    pub fn message_counts(&self) -> Result<HashMap<i64, i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT folder_id, COUNT(*) FROM messages GROUP BY folder_id")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn count_messages(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?)
    }
}

/// What the contact pane knows about a correspondent.
#[derive(Debug, Clone, Default)]
pub struct SenderStats {
    pub messages: i64,
    pub first: Option<DateTime<Utc>>,
    pub last: Option<DateTime<Utc>>,
}

/// Midnight this morning, local time, as a Unix timestamp. Messages are
/// stored in UTC, so "today" has to be resolved against the running clock
/// rather than baked into the SQL.
fn local_day_start() -> i64 {
    chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| naive.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

/// Read a `MessageSummary` out of a row whose columns start at `base` in the
/// order every summary query above selects them. One function so the list,
/// the contact pane and the reading pane cannot drift apart.
fn summary_from_row(row: &rusqlite::Row<'_>, base: usize) -> rusqlite::Result<MessageSummary> {
    let ts: Option<i64> = row.get(base + 6)?;
    Ok(MessageSummary {
        id: row.get(base)?,
        account_email: row.get(base + 1)?,
        folder_name: row.get(base + 2)?,
        uid: row.get::<_, i64>(base + 3)?.clamp(0, u32::MAX as i64) as u32,
        subject: row.get(base + 4)?,
        from: row.get(base + 5)?,
        date: ts.and_then(|t| DateTime::<Utc>::from_timestamp(t, 0)),
        seen: row.get::<_, i64>(base + 7)? != 0,
        flagged: row.get::<_, i64>(base + 8)? != 0,
        has_attachments: row.get::<_, i64>(base + 9)? != 0,
        preview: row.get::<_, String>(base + 10)?.trim().to_string(),
    })
}

fn smtp_security_text(value: SmtpSecurity) -> &'static str {
    match value {
        SmtpSecurity::Tls => "tls",
        SmtpSecurity::StartTls => "start_tls",
    }
}

/// Unknown values fall back to implicit TLS, which is what every account
/// written before the column existed was using.
fn smtp_security_from(value: &str) -> SmtpSecurity {
    match value {
        "start_tls" => SmtpSecurity::StartTls,
        _ => SmtpSecurity::Tls,
    }
}
