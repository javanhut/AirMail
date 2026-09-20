pub mod composer;
pub mod mailbox;
pub mod setup;

use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config;
use crate::db::Db;
use crate::models::{AccountConfig, Folder, MessageDetail, MessageSummary};
use crate::sync::{start_sync, SyncEvent};

use composer::{ComposerState, SendRequest};
use setup::SetupDialog;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Unified,
    Folder(i64, String),
}

pub struct AirMailApp {
    db: Db,
    db_path: PathBuf,
    accounts: Vec<AccountConfig>,
    runtime: tokio::runtime::Runtime,
    sync_handles: Vec<JoinHandle<()>>,
    sync_rx: mpsc::UnboundedReceiver<SyncEvent>,
    send_rx: mpsc::UnboundedReceiver<SendOutcome>,
    send_tx: mpsc::UnboundedSender<SendOutcome>,

    view: View,
    folders_cache: Vec<(String, Vec<Folder>)>,
    summaries: Vec<MessageSummary>,
    detail: Option<MessageDetail>,
    setup: Option<SetupDialog>,
    composer: Option<ComposerState>,
    status: String,
}

struct SendOutcome {
    account_email: String,
    result: Result<(), String>,
}

impl AirMailApp {
    pub fn new() -> Result<Self> {
        let db_path = config::db_path()?;
        let db = Db::open(&db_path)?;
        let accounts = config::load_accounts()?;
        let runtime = tokio::runtime::Runtime::new().context("creating tokio runtime")?;
        let (sync_rx, sync_handles) =
            start_sync(db_path.clone(), std::sync::Arc::new(accounts.clone()));
        let (send_tx, send_rx) = mpsc::unbounded_channel();
        let mut app = Self {
            db,
            db_path,
            accounts,
            runtime,
            sync_handles,
            sync_rx,
            send_rx,
            send_tx,
            view: View::Unified,
            folders_cache: Vec::new(),
            summaries: Vec::new(),
            detail: None,
            setup: None,
            composer: None,
            status: "Ready".to_string(),
        };
        app.refresh_folders();
        app.refresh_summaries();
        if app.accounts.is_empty() {
            app.setup = Some(SetupDialog::default());
        }
        Ok(app)
    }

    fn refresh_folders(&mut self) {
        self.folders_cache = self
            .db
            .list_accounts()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| {
                self.db
                    .folders(a.id)
                    .ok()
                    .map(|folders| (a.config.email.clone(), folders))
            })
            .collect();
    }

    fn refresh_summaries(&mut self) {
        let folder = match self.view {
            View::Unified => None,
            View::Folder(id, _) => Some(id),
        };
        match self.db.message_summaries(folder, 500) {
            Ok(rows) => self.summaries = rows,
            Err(e) => self.status = format!("query failed: {e:#}"),
        }
    }

    pub(crate) fn detail_is(&self, id: &i64) -> bool {
        self.detail.as_ref().is_some_and(|d| d.summary.id == *id)
    }

    /// Re-query the list but keep the open message selected if it still exists.
    pub(crate) fn refresh_summaries_keep_selection(&mut self) {
        let selected = self.detail.as_ref().map(|d| d.summary.id);
        self.refresh_summaries();
        if let Some(id) = selected {
            match self.db.message_detail(id) {
                Ok(Some(detail)) => self.detail = Some(detail),
                _ => self.detail = None,
            }
        }
    }

    fn restart_sync(&mut self) {        for handle in self.sync_handles.drain(..) {
            handle.abort();
        }
        self.accounts = config::load_accounts().unwrap_or_default();
        let (sync_rx, handles) =
            start_sync(self.db_path.clone(), std::sync::Arc::new(self.accounts.clone()));
        self.sync_rx = sync_rx;
        self.sync_handles = handles;
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        while let Ok(event) = self.sync_rx.try_recv() {
            match event {
                SyncEvent::Updated { .. } | SyncEvent::NewMessages { .. } => changed = true,
                SyncEvent::Error { account, message } => {
                    self.status = format!("sync error for {account}: {message}");
                }
            }
        }
        if changed {
            self.refresh_folders();
            self.refresh_summaries();
        }
        while let Ok(outcome) = self.send_rx.try_recv() {
            match outcome.result {
                Ok(()) => {
                    self.status = format!("message sent via {}", outcome.account_email);
                    self.refresh_folders();
                    self.refresh_summaries();
                }
                Err(e) => self.status = format!("send failed: {e}"),
            }
        }
        // Cheap wake-up for channel polling; sync workers push at most every few seconds.
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn spawn_send(&mut self, request: SendRequest) {
        let send_tx = self.send_tx.clone();
        let account_email = request.account_email.clone();
        self.runtime.spawn(async move {
            let result = send_task(request).await.map_err(|e| format!("{e:#}"));
            let _ = send_tx.send(SendOutcome {
                account_email,
                result,
            });
        });
    }
}

/// SMTP send, then try to APPEND the copy to the Sent folder (best effort).
async fn send_task(request: SendRequest) -> Result<()> {
    let cfg = config::load_accounts()?
        .into_iter()
        .find(|a| a.email == request.account_email)
        .with_context(|| format!("account {:?} no longer exists", request.account_email))?;
    let password = config::get_password(&cfg.email)?;

    let sent = crate::smtp::send(&cfg, &password, &request.to, &request.subject, &request.body)
        .await
        .context("sending")?;

    match crate::sync::imap::connect(&cfg, &password).await {
        Ok(mut session) => {
            let db = tokio::task::spawn_blocking({
                let db_path = config::db_path();
                move || db_path.and_then(|p| crate::db::Db::open(&p))
            })
            .await
            .context("opening database")??;
            let sent_folder = db
                .list_accounts()?
                .into_iter()
                .find(|a| a.config.email == cfg.email)
                .and_then(|a| a.sent_folder);
            if let Some(folder) = sent_folder
                && let Err(e) =
                    crate::sync::imap::append_to_folder(&mut session, &folder, &sent.raw).await
            {
                tracing::warn!("could not copy to Sent folder: {e:#}");
            }
            let _ = session.logout().await;
        }
        Err(e) => tracing::warn!("could not connect to append Sent copy: {e:#}"),
    }
    Ok(())
}

impl eframe::App for AirMailApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_events(&ctx);

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.accounts.is_empty() && ui.button("New message").clicked() {
                        self.composer = Some(ComposerState::new(&self.accounts));
                    }
                    if ui.button("+ Add account").clicked() {
                        self.setup = Some(SetupDialog::default());
                    }
                });
            });
        });

        egui::Panel::left("accounts")
            .resizable(true)
            .default_size(190.0)
            .show(ui, |ui| mailbox::show_accounts_panel(self, ui));

        if !self.summaries.is_empty() || matches!(self.view, View::Folder(..)) {
            egui::Panel::left("messages")
                .resizable(true)
                .default_size(300.0)
                .show(ui, |ui| mailbox::show_message_list(self, ui));
        }

        egui::CentralPanel::default().show(ui, |ui| mailbox::show_reading_pane(self, ui));

        if let Some(setup) = self.setup.as_mut() {
            let mut open = true;
            egui::Window::new("Add account")
                .open(&mut open)
                .collapsible(false)
                .show(&ctx, |ui| setup.show(ui));
            if !open {
                self.setup = None;
            }
        }
        if let Some(setup) = self.setup.take_if(|s| s.wants_save()) {
            match setup.save() {
                Ok((cfg, password)) => {
                    if let Err(e) = config::store_password(&cfg.email, &password) {
                        self.status = format!("account saved, but password storage failed: {e:#}");
                    }
                    if let Err(e) = self.db.upsert_account(&cfg) {
                        self.status = format!("account saved, but database failed: {e:#}");
                    }
                    self.restart_sync();
                    self.refresh_folders();
                    self.refresh_summaries();
                    self.status = format!("account {} added", cfg.email);
                }
                Err(e) => self.status = format!("could not add account: {e:#}"),
            }
        }

        let mut send_request = None;
        let mut composer_done = false;
        if let Some(composer) = self.composer.as_mut() {
            let mut open = true;
            egui::Window::new("New message")
                .open(&mut open)
                .collapsible(false)
                .default_size(egui::vec2(520.0, 420.0))
                .show(&ctx, |ui| composer.show(ui, &mut send_request));
            composer_done = !open || send_request.is_some();
        }
        if composer_done {
            self.composer = None;
        }
        if let Some(request) = send_request {
            self.spawn_send(request);
            self.status = "sending...".to_string();
        }
    }
}

pub fn run() -> Result<()> {
    let app = AirMailApp::new()?;
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "AirMail",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}
