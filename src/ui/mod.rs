pub mod composer;
pub mod contact;
pub mod mailbox;
pub mod message_object;
pub mod setup;
pub mod theme;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config;
use crate::db::Db;
use crate::models::{AccountConfig, Folder, MessageDetail, MessageSummary};
use crate::sync::{SyncEvent, start_sync};

use composer::{Prefill, SendRequest};

const APP_ID: &str = "dev.raven.AirMail";

/// How many rows one view pulls out of the database at a time.
const PAGE: usize = 800;

/// The mailboxes down the top of the sidebar. These are views over whatever
/// folders the servers actually have rather than folders in their own right,
/// which is why an account with no Archive simply shows an empty one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Smart {
    Inbox,
    Today,
    Starred,
    Snoozed,
    Sent,
    Drafts,
    Archive,
    Trash,
}

impl Smart {
    pub const ALL: [Smart; 8] = [
        Smart::Inbox,
        Smart::Today,
        Smart::Starred,
        Smart::Snoozed,
        Smart::Sent,
        Smart::Drafts,
        Smart::Archive,
        Smart::Trash,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Smart::Inbox => "Inbox",
            Smart::Today => "Today",
            Smart::Starred => "Starred",
            Smart::Snoozed => "Snoozed",
            Smart::Sent => "Sent",
            Smart::Drafts => "Drafts",
            Smart::Archive => "Archive",
            Smart::Trash => "Trash",
        }
    }

    /// Adwaita's symbolic set has no inbox or tag icon, so a few of these are
    /// the nearest thing that reads right at 16px rather than an exact name.
    pub fn icon(self) -> &'static str {
        match self {
            Smart::Inbox => "mail-unread-symbolic",
            Smart::Today => "daytime-sunrise-symbolic",
            Smart::Starred => "starred-symbolic",
            Smart::Snoozed => "alarm-symbolic",
            Smart::Sent => "mail-send-symbolic",
            Smart::Drafts => "document-edit-symbolic",
            Smart::Archive => "folder-download-symbolic",
            Smart::Trash => "user-trash-symbolic",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Smart(Smart),
    /// Everything belonging to one account, whatever folder it is in.
    Account(String),
    Folder(i64, String),
}

/// The chips over the message list. They narrow whatever the view selected
/// rather than replacing it, so "Inbox + Unread" is a thing you can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Unread,
    Starred,
    Attachments,
}

impl Filter {
    pub const ALL: [Filter; 4] = [
        Filter::All,
        Filter::Unread,
        Filter::Starred,
        Filter::Attachments,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Unread => "Unread",
            Filter::Starred => "Starred",
            Filter::Attachments => "Attachments",
        }
    }

    fn matches(self, message: &MessageSummary) -> bool {
        match self {
            Filter::All => true,
            Filter::Unread => !message.seen,
            Filter::Starred => message.flagged,
            Filter::Attachments => message.has_attachments,
        }
    }
}

/// What a server folder is for. IMAP has no portable way to ask, so the name
/// is all there is to go on — which is what every other client does too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    Inbox,
    Sent,
    Drafts,
    Archive,
    Trash,
    Junk,
    /// Anything the user made themselves: a label, in Gmail's telling.
    Label,
}

pub fn folder_kind(name: &str) -> FolderKind {
    let leaf = mailbox::leaf_name(name).to_ascii_lowercase();
    if leaf == "inbox" {
        FolderKind::Inbox
    } else if leaf.contains("sent") {
        FolderKind::Sent
    } else if leaf.contains("draft") {
        FolderKind::Drafts
    } else if leaf.contains("trash") || leaf.contains("deleted") || leaf == "bin" {
        FolderKind::Trash
    } else if leaf.contains("junk") || leaf.contains("spam") {
        FolderKind::Junk
    } else if leaf.contains("archive") || leaf == "all mail" {
        FolderKind::Archive
    } else {
        FolderKind::Label
    }
}

/// Everything the window shows, with no widgets in it.
///
/// Keeping state and widgets apart is what makes the GTK port tractable: a
/// callback borrows this, decides what changed, drops the borrow, and only
/// then touches widgets — so a signal fired by our own update can never
/// re-enter a live `RefMut`.
pub struct AppState {
    db: Db,
    db_path: PathBuf,
    pub accounts: Vec<AccountConfig>,
    runtime: tokio::runtime::Runtime,
    sync_handles: Vec<JoinHandle<()>>,
    send_tx: mpsc::UnboundedSender<SendOutcome>,

    pub view: View,
    pub filter: Filter,
    pub newest_first: bool,
    /// Whether the sidebar shows the raw per-account folder tree under
    /// "More", which is the only way to reach a folder no smart view claims.
    pub show_all_folders: bool,
    pub folders_cache: Vec<(String, Vec<Folder>)>,
    pub unread: HashMap<i64, i64>,
    pub totals: HashMap<i64, i64>,
    pub starred_total: i64,
    pub unread_today: i64,
    pub summaries: Vec<MessageSummary>,
    pub detail: Option<MessageDetail>,
    /// Text typed into the search field; filters the visible list.
    pub search: String,
    pub status: String,
}

pub struct SendOutcome {
    account_email: String,
    result: Result<(), String>,
}

/// What `rebuild_sidebar` needs, read out of the state in one borrow so the
/// rebuild itself can touch widgets (and re-enter the state) freely.
pub struct SidebarSnapshot {
    pub view: View,
    pub show_all_folders: bool,
    pub smart: Vec<(Smart, i64)>,
    pub accounts: Vec<SidebarAccount>,
    pub labels: Vec<(i64, String)>,
}

/// One account as the sidebar lists it, with the folder tree "More" opens.
pub struct SidebarAccount {
    pub email: String,
    pub unread: i64,
    pub folders: Vec<SidebarFolder>,
}

pub struct SidebarFolder {
    pub id: i64,
    pub name: String,
    pub unread: i64,
}

/// Handles to the widgets the state writes into. Cloned into every callback,
/// so it lives behind an `Rc` and holds nothing but GTK objects (which are
/// refcounted themselves).
pub struct Widgets {
    pub window: adw::ApplicationWindow,
    pub toasts: adw::ToastOverlay,
    pub search: gtk::Entry,
    pub compose_button: gtk::Button,
    pub sidebar: gtk::Box,
    pub chips: Vec<(Filter, gtk::Button)>,
    pub sort_button: gtk::Button,
    pub message_store: gtk::gio::ListStore,
    pub message_selection: gtk::SingleSelection,
    pub message_placeholder: gtk::Stack,
    pub reading: mailbox::ReadingPane,
    pub contact: contact::ContactPane,
    pub details_split: adw::OverlaySplitView,
    pub status_bar: gtk::Box,
    pub status: gtk::Label,
    pub unread_total: gtk::Label,
}

pub type Ui = Rc<Widgets>;

impl AppState {
    /// Open the database and start syncing. Returns the receivers the GTK main
    /// loop pumps, because they belong to the loop rather than to the state.
    pub fn new() -> Result<(
        Self,
        mpsc::UnboundedReceiver<SyncEvent>,
        mpsc::UnboundedReceiver<SendOutcome>,
    )> {
        let db_path = config::db_path()?;
        let db = Db::open(&db_path)?;
        let accounts = config::load_accounts()?;
        let runtime = tokio::runtime::Runtime::new().context("creating tokio runtime")?;
        // `start_sync` calls `tokio::spawn`, which panics unless a runtime is
        // current on this thread — and the GTK main loop is not one.
        let (sync_rx, sync_handles) = {
            let _guard = runtime.enter();
            start_sync(db_path.clone(), std::sync::Arc::new(accounts.clone()))
        };
        let (send_tx, send_rx) = mpsc::unbounded_channel();
        let mut state = Self {
            db,
            db_path,
            accounts,
            runtime,
            sync_handles,
            send_tx,
            view: View::Smart(Smart::Inbox),
            filter: Filter::All,
            newest_first: true,
            show_all_folders: false,
            folders_cache: Vec::new(),
            unread: HashMap::new(),
            totals: HashMap::new(),
            starred_total: 0,
            unread_today: 0,
            summaries: Vec::new(),
            detail: None,
            search: String::new(),
            status: String::new(),
        };
        state.refresh_folders();
        state.refresh_summaries();
        Ok((state, sync_rx, send_rx))
    }

    pub fn refresh_folders(&mut self) {
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
        self.unread = self.db.unread_counts().unwrap_or_default();
        self.totals = self.db.message_counts().unwrap_or_default();
        self.starred_total = self.db.count_flagged().unwrap_or(0);
        self.unread_today = self.db.unread_today().unwrap_or(0);
    }

    /// Ids of every folder the current view draws from, or `None` for "all of
    /// them" — which is what the date- and flag-based views want.
    fn scope(&self) -> Option<Vec<i64>> {
        match &self.view {
            View::Smart(Smart::Today | Smart::Starred) => None,
            View::Smart(Smart::Snoozed) => Some(Vec::new()),
            View::Smart(smart) => {
                let kind = match smart {
                    Smart::Inbox => FolderKind::Inbox,
                    Smart::Sent => FolderKind::Sent,
                    Smart::Drafts => FolderKind::Drafts,
                    Smart::Archive => FolderKind::Archive,
                    Smart::Trash => FolderKind::Trash,
                    // Handled above; kept exhaustive so a new mailbox has to
                    // say what it selects rather than silently showing all.
                    Smart::Today | Smart::Starred | Smart::Snoozed => return None,
                };
                Some(self.folder_ids_of_kind(kind))
            }
            View::Account(email) => Some(
                self.folders_cache
                    .iter()
                    .filter(|(account, _)| account == email)
                    .flat_map(|(_, folders)| folders.iter().map(|f| f.id))
                    .collect(),
            ),
            View::Folder(id, _) => Some(vec![*id]),
        }
    }

    fn folder_ids_of_kind(&self, kind: FolderKind) -> Vec<i64> {
        self.folders_cache
            .iter()
            .flat_map(|(_, folders)| folders)
            .filter(|f| folder_kind(&f.name) == kind)
            .map(|f| f.id)
            .collect()
    }

    pub fn folder_id_by_name(&self, name: &str) -> Option<i64> {
        self.folders_cache
            .iter()
            .flat_map(|(_, folders)| folders)
            .find(|f| f.name == name)
            .map(|f| f.id)
    }

    pub fn refresh_summaries(&mut self) {
        let scope = self.scope();
        let rows = match self.db.message_summaries(scope.as_deref(), PAGE) {
            Ok(rows) => rows,
            Err(e) => {
                self.status = format!("query failed: {e:#}");
                Vec::new()
            }
        };
        // Today and Starred select on the message rather than on its folder,
        // so they are narrowed here instead of in the query's scope.
        self.summaries = match self.view {
            View::Smart(Smart::Today) => {
                let today = chrono::Local::now().date_naive();
                rows.into_iter()
                    .filter(|m| {
                        m.date
                            .is_some_and(|d| d.with_timezone(&chrono::Local).date_naive() == today)
                    })
                    .collect()
            }
            View::Smart(Smart::Starred) => rows.into_iter().filter(|m| m.flagged).collect(),
            _ => rows,
        };
    }

    pub fn detail_is(&self, id: i64) -> bool {
        self.detail.as_ref().is_some_and(|d| d.summary.id == id)
    }

    /// Unread total for one account, summed over its folders.
    pub fn unread_for_account(&self, email: &str) -> i64 {
        self.folders_cache
            .iter()
            .filter(|(account, _)| account == email)
            .flat_map(|(_, folders)| folders)
            .filter_map(|f| self.unread.get(&f.id))
            .sum()
    }

    /// The rows the list should show: whatever the view selected, narrowed by
    /// the chip and the search field, in the chosen order.
    pub fn visible_summaries(&self) -> Vec<&MessageSummary> {
        let needle = self.search.trim().to_lowercase();
        let mut rows: Vec<&MessageSummary> = self
            .summaries
            .iter()
            .filter(|m| self.filter.matches(m))
            .filter(|m| {
                needle.is_empty()
                    || m.subject.to_lowercase().contains(&needle)
                    || m.from.to_lowercase().contains(&needle)
                    || m.preview.to_lowercase().contains(&needle)
                    || m.account_email.to_lowercase().contains(&needle)
                    || m.folder_name.to_lowercase().contains(&needle)
            })
            .collect();
        // The query already came back newest first; the other order is this
        // page of messages reversed, not an older page.
        if !self.newest_first {
            rows.reverse();
        }
        rows
    }

    /// What the list says when it has nothing to show. Worth being specific:
    /// an empty Snoozed means something different from an empty search.
    pub fn empty_text(&self) -> &'static str {
        if self.view == View::Smart(Smart::Snoozed) {
            return "Nothing snoozed — snoozing isn't available yet.";
        }
        if !self.search.trim().is_empty() {
            return "No messages match that search.";
        }
        match self.filter {
            Filter::All => "Nothing here yet.",
            Filter::Unread => "Nothing unread here.",
            Filter::Starred => "Nothing starred here.",
            Filter::Attachments => "Nothing here has an attachment.",
        }
    }

    pub fn sidebar_snapshot(&self) -> SidebarSnapshot {
        let unread_of_kind = |kind: FolderKind| -> i64 {
            self.folder_ids_of_kind(kind)
                .iter()
                .filter_map(|id| self.unread.get(id))
                .sum()
        };
        let total_of_kind = |kind: FolderKind| -> i64 {
            self.folder_ids_of_kind(kind)
                .iter()
                .filter_map(|id| self.totals.get(id))
                .sum()
        };

        let smart = Smart::ALL
            .iter()
            .map(|smart| {
                let count = match smart {
                    Smart::Inbox => unread_of_kind(FolderKind::Inbox),
                    Smart::Today => self.unread_today,
                    Smart::Starred => self.starred_total,
                    Smart::Snoozed => 0,
                    Smart::Sent => 0,
                    // A draft is written, not received, so an unread count
                    // would always be nought — the total is the useful number.
                    Smart::Drafts => total_of_kind(FolderKind::Drafts),
                    Smart::Archive => unread_of_kind(FolderKind::Archive),
                    Smart::Trash => unread_of_kind(FolderKind::Trash),
                };
                (*smart, count)
            })
            .collect();

        let accounts: Vec<SidebarAccount> = self
            .folders_cache
            .iter()
            .map(|(email, folders)| SidebarAccount {
                email: email.clone(),
                unread: self.unread_for_account(email),
                folders: folders
                    .iter()
                    .map(|f| SidebarFolder {
                        id: f.id,
                        name: f.name.clone(),
                        unread: self.unread.get(&f.id).copied().unwrap_or(0),
                    })
                    .collect(),
            })
            .collect();

        // One entry per label name, so the same label on two accounts is one
        // line — which is what it looks like to the person reading it.
        let mut labels: Vec<(i64, String)> = Vec::new();
        for folder in self.folders_cache.iter().flat_map(|(_, f)| f) {
            if folder_kind(&folder.name) == FolderKind::Label
                && !labels.iter().any(|(_, name)| name == &folder.name)
            {
                labels.push((folder.id, folder.name.clone()));
            }
        }
        labels.sort_by(|a, b| a.1.cmp(&b.1));

        SidebarSnapshot {
            view: self.view.clone(),
            show_all_folders: self.show_all_folders,
            smart,
            accounts,
            labels,
        }
    }

    /// Re-query the list but keep the open message selected if it still exists.
    pub fn refresh_summaries_keep_selection(&mut self) {
        let selected = self.detail.as_ref().map(|d| d.summary.id);
        self.refresh_summaries();
        if let Some(id) = selected {
            match self.db.message_detail(id) {
                Ok(Some(detail)) => self.detail = Some(detail),
                _ => self.detail = None,
            }
        }
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Stop the running sync loops and start fresh ones for the accounts on
    /// disk. The caller pumps the returned receiver; the old one ends by
    /// itself once the aborted tasks drop their senders.
    #[must_use]
    pub fn restart_sync(&mut self) -> mpsc::UnboundedReceiver<SyncEvent> {
        for handle in self.sync_handles.drain(..) {
            handle.abort();
        }
        self.accounts = config::load_accounts().unwrap_or_default();
        let _guard = self.runtime.enter();
        let (sync_rx, handles) = start_sync(
            self.db_path.clone(),
            std::sync::Arc::new(self.accounts.clone()),
        );
        self.sync_handles = handles;
        sync_rx
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

    let sent = crate::smtp::send(
        &cfg,
        &password,
        &request.to,
        &request.subject,
        &request.body,
    )
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

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

/// Redraw everything that depends on state: sidebar, list, reading pane and
/// the status line. Cheap enough to call after any change — the message list
/// rebuilds a model of plain objects, not widgets.
pub fn refresh_all(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    mailbox::rebuild_sidebar(state, ui);
    mailbox::rebuild_message_list(state, ui);
    mailbox::update_reading_pane(state, ui);
    update_status(state, ui);
}

pub fn update_status(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    let state = state.borrow();
    ui.status.set_text(&state.status);
    let total: i64 = state.unread.values().sum();
    ui.unread_total.set_text(&match total {
        0 => String::new(),
        n => format!("{n} unread"),
    });
    // The design has no footer, and at rest there is nothing to put in one.
    // It appears only when there is something to say.
    ui.status_bar.set_visible(!state.status.is_empty());
    ui.compose_button.set_sensitive(!state.accounts.is_empty());
}

/// Set the status line and, for things worth noticing, raise a toast.
pub fn set_status(state: &Rc<RefCell<AppState>>, ui: &Ui, message: impl Into<String>, toast: bool) {
    let message = message.into();
    state.borrow_mut().status = message.clone();
    if toast {
        ui.toasts.add_toast(adw::Toast::new(&message));
    }
    update_status(state, ui);
}

/// Drain sync events as they arrive. Replaces egui's per-frame `try_recv`
/// polling: the loop parks on the channel and wakes the main context only when
/// a worker actually pushes something.
pub fn pump_sync_events(
    mut rx: mpsc::UnboundedReceiver<SyncEvent>,
    state: &Rc<RefCell<AppState>>,
    ui: &Ui,
) {
    let state = state.clone();
    let ui = ui.clone();
    gtk::glib::MainContext::default().spawn_local(async move {
        while let Some(event) = rx.recv().await {
            let mut changed = false;
            let mut error = None;
            match event {
                SyncEvent::Updated { .. } | SyncEvent::NewMessages { .. } => changed = true,
                SyncEvent::Error { account, message } => {
                    error = Some(format!("sync error for {account}: {message}"));
                }
            }
            if changed {
                {
                    let mut state = state.borrow_mut();
                    state.refresh_folders();
                    state.refresh_summaries_keep_selection();
                }
                refresh_all(&state, &ui);
            }
            if let Some(error) = error {
                set_status(&state, &ui, error, true);
            }
        }
    });
}

/// The same, for the outcome of a send.
pub fn pump_send_outcomes(
    mut rx: mpsc::UnboundedReceiver<SendOutcome>,
    state: &Rc<RefCell<AppState>>,
    ui: &Ui,
) {
    let state = state.clone();
    let ui = ui.clone();
    gtk::glib::MainContext::default().spawn_local(async move {
        while let Some(outcome) = rx.recv().await {
            match outcome.result {
                Ok(()) => {
                    {
                        let mut state = state.borrow_mut();
                        state.refresh_folders();
                        state.refresh_summaries_keep_selection();
                    }
                    refresh_all(&state, &ui);
                    set_status(
                        &state,
                        &ui,
                        format!("Message sent from {}", outcome.account_email),
                        true,
                    );
                }
                Err(e) => set_status(&state, &ui, format!("Send failed: {e}"), true),
            }
        }
    });
}

/// Persist a finished setup dialog: TOML on disk, password in the keyring,
/// row in the database, then restart syncing so mail starts arriving.
pub fn add_account(state: &Rc<RefCell<AppState>>, ui: &Ui, cfg: &AccountConfig, password: &str) {
    let stored = {
        let state_ref = state.borrow();
        config::store_password(&cfg.email, password)
            .map_err(|e| format!("Could not store the password: {e:#}"))
            .and_then(|()| {
                config::save_account(cfg).map_err(|e| format!("Could not save the account: {e:#}"))
            })
            .and_then(|()| {
                state_ref
                    .db
                    .upsert_account(cfg)
                    .map(|_| ())
                    .map_err(|e| format!("Could not record the account: {e:#}"))
            })
    };
    if let Err(e) = stored {
        set_status(state, ui, e, true);
        return;
    }

    let sync_rx = {
        let mut state = state.borrow_mut();
        let rx = state.restart_sync();
        state.refresh_folders();
        state.refresh_summaries();
        rx
    };
    pump_sync_events(sync_rx, state, ui);
    refresh_all(state, ui);
    set_status(state, ui, format!("Added {} — syncing", cfg.email), true);
}

/// Forget an account: config file, keyring entry and every cached message.
pub fn remove_account(state: &Rc<RefCell<AppState>>, ui: &Ui, email: &str) {
    let removed =
        config::delete_account(email).and_then(|()| state.borrow().db.remove_account(email));
    if let Err(e) = removed {
        set_status(
            state,
            ui,
            format!("Removing the account failed: {e:#}"),
            true,
        );
        return;
    }

    let sync_rx = {
        let mut state = state.borrow_mut();
        let rx = state.restart_sync();
        if state.accounts.iter().all(|a| a.email != email) {
            state.view = View::Smart(Smart::Inbox);
            state.detail = None;
        }
        state.refresh_folders();
        state.refresh_summaries();
        rx
    };
    pump_sync_events(sync_rx, state, ui);
    refresh_all(state, ui);
    set_status(state, ui, format!("Removed {email}"), true);
}

/// Removing an account also drops its cached mail and its keyring entry, and
/// none of that comes back, so it asks first.
pub fn confirm_removal(state: &Rc<RefCell<AppState>>, ui: &Ui, email: &str) {
    let dialog = adw::AlertDialog::new(
        Some("Remove account"),
        Some(
            "Its stored password and downloaded mail are deleted from this computer. \
             Nothing on the server changes.",
        ),
    );
    dialog.set_heading(Some(&format!("Remove {email}?")));
    dialog.add_responses(&[("cancel", "Cancel"), ("remove", "Remove")]);
    dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    let window = ui.window.clone();
    let state = state.clone();
    let ui = ui.clone();
    let email = email.to_string();
    dialog.connect_response(None, move |_, response| {
        if response == "remove" {
            remove_account(&state, &ui, &email);
        }
    });
    dialog.present(Some(&window));
}

pub fn open_composer(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    open_composer_with(state, ui, Prefill::default());
}

/// Reply to whatever is open, with the quote already in the body.
pub fn open_reply(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    let Some(detail) = state.borrow().detail.clone() else {
        return;
    };
    open_composer_with(state, ui, Prefill::reply_to(&detail));
}

fn open_composer_with(state: &Rc<RefCell<AppState>>, ui: &Ui, prefill: Prefill) {
    let accounts: Vec<String> = state
        .borrow()
        .accounts
        .iter()
        .map(|a| a.email.clone())
        .collect();
    if accounts.is_empty() {
        return;
    }
    let state = state.clone();
    let ui_for_send = ui.clone();
    composer::present(&accounts, prefill, ui, move |request| {
        state.borrow_mut().spawn_send(request);
        set_status(&state, &ui_for_send, "Sending…", false);
    });
}

pub fn open_setup(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    let state = state.clone();
    let ui_for_save = ui.clone();
    setup::present(ui, move |cfg, password| {
        add_account(&state, &ui_for_save, &cfg, &password);
    });
}

/// Put the given text in the search field, which filters the list through the
/// field's own `changed` handler.
pub fn search_for(ui: &Ui, text: &str) {
    ui.search.set_text(text);
    ui.search.grab_focus_without_selecting();
    ui.search.set_position(-1);
}

pub fn copy_to_clipboard(ui: &Ui, text: &str) {
    ui.window.clipboard().set_text(text);
}

fn build_window(app: &adw::Application, state: &Rc<RefCell<AppState>>) -> Ui {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("AirMail")
        .default_width(1480)
        .default_height(940)
        .width_request(940)
        .height_request(600)
        .build();
    window.add_css_class("airmail");

    let header = mailbox::build_header();
    let (sidebar_page, sidebar, compose_sidebar) = mailbox::build_sidebar();
    let list = mailbox::build_message_list();
    let (reading_page, reading) = mailbox::build_reading_pane();
    let contact = contact::build();

    // The contact card is an overlay split rather than a third navigation
    // level: it is a detail of the open message, not somewhere you navigate
    // to, and this way it can be folded away without losing your place.
    let details_split = adw::OverlaySplitView::new();
    details_split.set_sidebar_position(gtk::PackType::End);
    details_split.set_sidebar(Some(&contact.root));
    details_split.set_content(Some(&reading_page));
    details_split.set_min_sidebar_width(230.0);
    details_split.set_max_sidebar_width(300.0);
    details_split.set_sidebar_width_fraction(0.2);

    let inner_split = adw::NavigationSplitView::new();
    inner_split.set_sidebar(Some(&list.page));
    inner_split.set_content(Some(&adw::NavigationPage::new(&details_split, "Message")));
    inner_split.set_min_sidebar_width(330.0);
    inner_split.set_max_sidebar_width(470.0);
    inner_split.set_sidebar_width_fraction(0.3);

    let outer_split = adw::NavigationSplitView::new();
    outer_split.set_sidebar(Some(&sidebar_page));
    outer_split.set_content(Some(&adw::NavigationPage::new(&inner_split, "Mail")));
    outer_split.set_min_sidebar_width(230.0);
    outer_split.set_max_sidebar_width(280.0);
    outer_split.set_sidebar_width_fraction(0.17);

    let (status_bar, status, unread_total) = mailbox::build_status_bar();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header.bar);
    toolbar.set_content(Some(&outer_split));
    toolbar.add_bottom_bar(&status_bar);

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&toolbar));
    window.set_content(Some(&toasts));

    let ui: Ui = Rc::new(Widgets {
        window: window.clone(),
        toasts,
        search: header.search.clone(),
        compose_button: header.compose.clone(),
        sidebar,
        chips: list.chips.clone(),
        sort_button: list.sort.clone(),
        message_store: list.store,
        message_selection: list.selection,
        message_placeholder: list.placeholder,
        reading,
        contact,
        details_split: details_split.clone(),
        status_bar,
        status,
        unread_total,
    });

    // Search filters the list without re-querying the database.
    {
        let state = state.clone();
        let ui = ui.clone();
        header.search.connect_changed(move |entry| {
            state.borrow_mut().search = entry.text().to_string();
            mailbox::rebuild_message_list(&state, &ui);
        });
    }

    // Filter chips and sort order.
    for (filter, button) in &ui.chips {
        let state = state.clone();
        let ui = ui.clone();
        let filter = *filter;
        button.connect_clicked(move |_| {
            state.borrow_mut().filter = filter;
            mailbox::rebuild_message_list(&state, &ui);
        });
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        ui.sort_button.clone().connect_clicked(move |_| {
            {
                let mut state = state.borrow_mut();
                state.newest_first = !state.newest_first;
            }
            mailbox::rebuild_message_list(&state, &ui);
        });
    }

    // Single-click activation, so a click opens the message the way the egui
    // rows did.
    {
        let state = state.clone();
        let ui = ui.clone();
        list.view.connect_activate(move |view, position| {
            let Some(object) = view
                .model()
                .and_then(|model| model.item(position))
                .and_downcast::<message_object::MessageObject>()
            else {
                return;
            };
            mailbox::open_message(&state, &ui, object.id());
        });
    }

    for button in [&header.compose, &compose_sidebar] {
        let state = state.clone();
        let ui = ui.clone();
        button.connect_clicked(move |_| open_composer(&state, &ui));
    }

    // Reading-pane actions that AirMail can actually carry out.
    {
        let state = state.clone();
        let ui = ui.clone();
        ui.reading
            .star
            .clone()
            .connect_clicked(move |_| mailbox::toggle_star(&state, &ui));
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        ui.reading
            .unread_button
            .clone()
            .connect_clicked(move |_| mailbox::mark_unread(&state, &ui));
    }

    // Contact card actions.
    {
        let state = state.clone();
        let ui = ui.clone();
        ui.contact
            .reply
            .clone()
            .connect_clicked(move |_| open_reply(&state, &ui));
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        ui.contact.compose.clone().connect_clicked(move |_| {
            let to = ui.contact.address.text().to_string();
            open_composer_with(
                &state,
                &ui,
                Prefill {
                    to,
                    ..Prefill::default()
                },
            );
        });
    }
    {
        let ui = ui.clone();
        ui.contact.copy.clone().connect_clicked(move |_| {
            let address = ui.contact.address.text().to_string();
            copy_to_clipboard(&ui, &address);
            ui.toasts
                .add_toast(adw::Toast::new(&format!("Copied {address}")));
        });
    }
    {
        let ui = ui.clone();
        ui.contact.find.clone().connect_clicked(move |_| {
            let address = ui.contact.address.text().to_string();
            search_for(&ui, &address);
        });
    }

    header.menu.set_popover(Some(&build_menu(state, &ui)));
    install_shortcuts(&window, state, &ui);

    ui
}

/// The header's "…" menu. A popover of flat buttons rather than a `GMenu`:
/// there are three entries, and this way they call the same functions every
/// other button does without a detour through actions.
fn build_menu(state: &Rc<RefCell<AppState>>, ui: &Ui) -> gtk::Popover {
    let popover = gtk::Popover::new();
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 2);
    theme::set_margins(&box_, 4);

    let sync_now = menu_item("view-refresh-symbolic", "Sync now");
    {
        let state = state.clone();
        let ui = ui.clone();
        let popover_ref = popover.clone();
        sync_now.connect_clicked(move |_| {
            popover_ref.popdown();
            let rx = state.borrow_mut().restart_sync();
            pump_sync_events(rx, &state, &ui);
            set_status(&state, &ui, "Syncing…", false);
        });
    }
    box_.append(&sync_now);

    let details = menu_item("sidebar-show-right-symbolic", "Hide contact details");
    {
        let ui = ui.clone();
        let popover_ref = popover.clone();
        let details_ref = details.clone();
        details.connect_clicked(move |_| {
            popover_ref.popdown();
            let showing = ui.details_split.shows_sidebar();
            ui.details_split.set_show_sidebar(!showing);
            if let Some(label) = details_ref
                .child()
                .and_downcast::<gtk::Box>()
                .and_then(|b| b.last_child())
                .and_downcast::<gtk::Label>()
            {
                label.set_text(if showing {
                    "Show contact details"
                } else {
                    "Hide contact details"
                });
            }
        });
    }
    box_.append(&details);

    let add = menu_item("list-add-symbolic", "Add an account…");
    {
        let state = state.clone();
        let ui = ui.clone();
        let popover_ref = popover.clone();
        add.connect_clicked(move |_| {
            popover_ref.popdown();
            open_setup(&state, &ui);
        });
    }
    box_.append(&add);

    let manage = menu_item("avatar-default-symbolic", "Manage accounts…");
    {
        let state = state.clone();
        let ui = ui.clone();
        let popover_ref = popover.clone();
        manage.connect_clicked(move |_| {
            popover_ref.popdown();
            open_accounts_dialog(&state, &ui);
        });
    }
    box_.append(&manage);

    popover.set_child(Some(&box_));
    popover
}

/// The accounts sheet. The sidebar lists accounts but, as in the design, has
/// no button to take one away — so removing one lives here, next to adding.
pub fn open_accounts_dialog(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Accounts");
    dialog.set_content_width(460);

    let header = adw::HeaderBar::new();
    let add = theme::primary_button("Add account");
    header.pack_end(&add);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    theme::set_margins(&content, 16);

    let emails: Vec<String> = state
        .borrow()
        .accounts
        .iter()
        .map(|a| a.email.clone())
        .collect();

    if emails.is_empty() {
        let empty = gtk::Label::new(Some("No accounts yet. Add one to start syncing."));
        empty.add_css_class("muted");
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        content.append(&empty);
    }

    for email in &emails {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.add_css_class("surface");
        theme::set_margins(&row, 10);
        row.append(&theme::avatar(email, 36));

        let words = gtk::Box::new(gtk::Orientation::Vertical, 1);
        words.set_hexpand(true);
        words.set_valign(gtk::Align::Center);
        let name = gtk::Label::new(Some(&mailbox::account_name(email)));
        name.set_xalign(0.0);
        let address = gtk::Label::new(Some(mailbox::strip_brackets(email)));
        address.add_css_class("small");
        address.add_css_class("faint");
        address.set_xalign(0.0);
        address.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        words.append(&name);
        words.append(&address);
        row.append(&words);

        let remove = theme::icon_button("user-trash-symbolic", &format!("Remove {email}"));
        {
            let state = state.clone();
            let ui = ui.clone();
            let email = email.clone();
            let dialog = dialog.clone();
            remove.connect_clicked(move |_| {
                dialog.close();
                confirm_removal(&state, &ui, &email);
            });
        }
        row.append(&remove);
        content.append(&row);
    }

    {
        let state = state.clone();
        let ui = ui.clone();
        let dialog = dialog.clone();
        add.connect_clicked(move |_| {
            dialog.close();
            open_setup(&state, &ui);
        });
    }

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(&ui.window));
}

fn menu_item(icon: &str, label: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(16);
    content.append(&image);
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    content.append(&text);

    let button = gtk::Button::builder().child(&content).build();
    button.add_css_class("flat");
    button.add_css_class("nav-row");
    button
}

/// Ctrl+K and Ctrl+N, the two shortcuts the window advertises on its face.
fn install_shortcuts(
    window: &adw::ApplicationWindow,
    state: &Rc<RefCell<AppState>>,
    ui: &Ui,
) {
    let controller = gtk::ShortcutController::new();
    controller.set_scope(gtk::ShortcutScope::Global);

    let ui_for_search = ui.clone();
    controller.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Control>k"),
        Some(gtk::CallbackAction::new(move |_, _| {
            ui_for_search.search.grab_focus();
            gtk::glib::Propagation::Stop
        })),
    ));

    let state_for_compose = state.clone();
    let ui_for_compose = ui.clone();
    controller.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Control>n"),
        Some(gtk::CallbackAction::new(move |_, _| {
            open_composer(&state_for_compose, &ui_for_compose);
            gtk::glib::Propagation::Stop
        })),
    ));

    window.add_controller(controller);
}

pub fn run() -> Result<()> {
    // Opening the database and starting the sync workers can fail, and it is
    // nicer to fail on the command line than inside `activate`.
    let (state, sync_rx, send_rx) = AppState::new()?;
    let state = Rc::new(RefCell::new(state));
    let startup = RefCell::new(Some((sync_rx, send_rx)));

    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| {
        theme::apply();
        let ui = build_window(app, &state);

        if let Some((sync_rx, send_rx)) = startup.borrow_mut().take() {
            pump_sync_events(sync_rx, &state, &ui);
            pump_send_outcomes(send_rx, &state, &ui);
        }

        refresh_all(&state, &ui);
        if state.borrow().accounts.is_empty() {
            open_setup(&state, &ui);
        }
        ui.window.present();
    });

    // AirMail parses its own arguments in `main`, so GTK is given none.
    let exit = app.run_with_args::<&str>(&[]);
    if exit == gtk::glib::ExitCode::SUCCESS {
        Ok(())
    } else {
        anyhow::bail!("GTK exited with {exit:?}")
    }
}
