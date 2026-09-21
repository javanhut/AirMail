use std::path::PathBuf;

use airmail::db::Db;
use airmail::models::{AccountConfig, SmtpSecurity};

fn temp_db(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("airmail-test-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

fn test_account(email: &str) -> AccountConfig {
    AccountConfig {
        email: email.to_string(),
        display_name: Some("Test User".to_string()),
        username: None,
        imap_host: "imap.example.com".to_string(),
        imap_port: 993,
        smtp_host: "smtp.example.com".to_string(),
        smtp_port: 465,
        smtp_security: SmtpSecurity::Tls,
    }
}

#[test]
fn account_roundtrip_and_cascade() {
    let path = temp_db("accounts");
    let db = Db::open(&path).unwrap();

    let id = db.upsert_account(&test_account("a@b.c")).unwrap();
    let accounts = db.list_accounts().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].config.email, "a@b.c");

    // upsert with same email updates in place
    let id2 = db.upsert_account(&test_account("a@b.c")).unwrap();
    assert_eq!(id, id2);
    assert_eq!(db.list_accounts().unwrap().len(), 1);

    let folder = db.upsert_folder(id, "INBOX", Some(42)).unwrap();
    db.store_message(
        folder, 7, "Hi", "x@y.z", "a@b.c", None, false, false, "body", "", b"raw",
    )
    .unwrap();
    assert_eq!(db.count_messages().unwrap(), 1);

    // deleting the account cascades to folders and messages
    db.remove_account("a@b.c").unwrap();
    assert_eq!(db.list_accounts().unwrap().len(), 0);
    assert_eq!(db.count_messages().unwrap(), 0);

    std::fs::remove_file(&path).ok();
}

#[test]
fn uidvalidity_change_purges_folder() {
    let path = temp_db("uidvalidity");
    let db = Db::open(&path).unwrap();

    let id = db.upsert_account(&test_account("v@w.x")).unwrap();
    let folder = db.upsert_folder(id, "INBOX", Some(1)).unwrap();
    db.store_message(folder, 1, "s", "f", "t", None, true, false, "b", "", b"r")
        .unwrap();
    assert_eq!(db.count_messages().unwrap(), 1);

    // server reports a new UIDVALIDITY → cache is wiped, resync from scratch
    db.upsert_folder(id, "INBOX", Some(2)).unwrap();
    assert_eq!(db.count_messages().unwrap(), 0);
    assert_eq!(db.folders(id).unwrap()[0].uid_validity, 2);

    std::fs::remove_file(&path).ok();
}

#[test]
fn message_summaries_unified_and_per_folder() {
    let path = temp_db("summaries");
    let db = Db::open(&path).unwrap();

    let id1 = db.upsert_account(&test_account("one@x.y")).unwrap();
    let id2 = db.upsert_account(&test_account("two@x.y")).unwrap();
    let f1 = db.upsert_folder(id1, "INBOX", Some(9)).unwrap();
    let f2 = db.upsert_folder(id2, "INBOX", Some(9)).unwrap();

    let ts = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    db.store_message(
        f1,
        1,
        "from one",
        "one@x.y",
        "",
        Some(ts),
        false,
        false,
        "",
        "",
        b"",
    )
    .unwrap();
    db.store_message(
        f2, 1, "from two", "two@x.y", "", None, true, true, "", "", b"",
    )
    .unwrap();

    let unified = db.message_summaries(None, 100).unwrap();
    assert_eq!(unified.len(), 2);
    assert!(unified.iter().any(|m| !m.seen));
    assert!(unified.iter().any(|m| m.has_attachments));

    let only_one = db.message_summaries(Some(f1), 100).unwrap();
    assert_eq!(only_one.len(), 1);
    assert_eq!(only_one[0].subject, "from one");

    let detail = db.message_detail(only_one[0].id).unwrap().unwrap();
    assert_eq!(detail.summary.from, "one@x.y");

    std::fs::remove_file(&path).ok();
}

#[test]
fn config_serializes_without_password() {
    let cfg = test_account("serde@x.y");
    let text = toml::to_string(&cfg).unwrap();
    assert!(text.contains("imap_host"));
    assert!(!text.contains("password"));
    let back: AccountConfig = toml::from_str(&text).unwrap();
    assert_eq!(back.email, cfg.email);
    assert_eq!(back.imap_port, 993);
}
