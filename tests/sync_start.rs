//! `start_sync` spawns onto the *current* tokio runtime, so the caller has to
//! have one entered. The GTK main loop is not a runtime, and the egui version
//! called this straight from `AirMailApp::new()` — which panicked as soon as
//! there was one account to sync. The UI now enters the runtime first; this
//! pins that down.

use std::path::PathBuf;
use std::sync::Arc;

use airmail::models::{AccountConfig, SmtpSecurity};
use airmail::sync::start_sync;

fn account() -> AccountConfig {
    AccountConfig {
        email: "regression@example.invalid".to_string(),
        display_name: None,
        username: None,
        imap_host: "imap.example.invalid".to_string(),
        imap_port: 993,
        smtp_host: "smtp.example.invalid".to_string(),
        smtp_port: 465,
        smtp_security: SmtpSecurity::Tls,
        oauth: None,
    }
}

#[test]
fn starting_sync_needs_the_runtime_entered() {
    // Current-thread, and never driven: the workers are queued but never get
    // to run, so nothing here touches the network or the keyring.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let (_events, handles) = {
        let _guard = runtime.enter();
        start_sync(
            PathBuf::from("/nonexistent/airmail.db"),
            Arc::new(vec![account()]),
        )
    };

    assert_eq!(handles.len(), 1, "one worker per account");
    for handle in handles {
        handle.abort();
    }
}
