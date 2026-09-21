pub mod composer;
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

use composer::SendRequest;

const APP_ID: &str = "dev.raven.AirMail";

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Unified,
    Folder(i64, String),
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
    pub folders_cache: Vec<(String, Vec<Folder>)>,
    pub unread: HashMap<i64, i64>,
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

/// Handles to the widgets the state writes into. Cloned into every callback,
/// so it lives behind an `Rc` and holds nothing but GTK objects (which are
/// refcounted themselves).
pub struct Widgets {
    pub window: adw::ApplicationWindow,
    pub toasts: adw::ToastOverlay,
    pub search: gtk::SearchEntry,
    pub compose_button: gtk::Button,
    pub sidebar: gtk::Box,
    pub list_title: gtk::Label,
    pub list_count: gtk::Label,
    pub message_store: gtk::gio::ListStore,
    pub message_selection: gtk::SingleSelection,
    pub message_placeholder: gtk::Stack,
    pub reading: mailbox::ReadingPane,
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
            view: View::Unified,
            folders_cache: Vec::new(),
            unread: HashMap::new(),
            summaries: Vec::new(),
            detail: None,
            search: String::new(),
            status: "Ready".to_string(),
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
    }

    pub fn refresh_summaries(&mut self) {
        let folder = match self.view {
            View::Unified => None,
            View::Folder(id, _) => Some(id),
        };
        match self.db.message_summaries(folder, 500) {
            Ok(rows) => self.summaries = rows,
            Err(e) => self.status = format!("query failed: {e:#}"),
        }
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

    /// The rows the list should show: everything, or whatever matches the
    /// search field.
    pub fn visible_summaries(&self) -> Vec<&MessageSummary> {
        let needle = self.search.trim().to_lowercase();
        if needle.is_empty() {
            return self.summaries.iter().collect();
        }
        self.summaries
            .iter()
            .filter(|m| {
                m.subject.to_lowercase().contains(&needle)
                    || m.from.to_lowercase().contains(&needle)
                    || m.account_email.to_lowercase().contains(&needle)
                    || m.folder_name.to_lowercase().contains(&needle)
            })
            .collect()
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
            state.view = View::Unified;
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
    composer::present(&accounts, ui, move |request| {
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

fn build_window(app: &adw::Application, state: &Rc<RefCell<AppState>>) -> Ui {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("AirMail")
        .default_width(1280)
        .default_height(820)
        .width_request(900)
        .height_request(560)
        .build();
    window.add_css_class("airmail");

    let (header, search, compose_button) = mailbox::build_header();
    let (sidebar_page, sidebar, add_account_button, compose_sidebar) = mailbox::build_sidebar();
    let (list_page, list_title, list_count, store, selection, placeholder, list_view) =
        mailbox::build_message_list();
    let (reading_page, reading) = mailbox::build_reading_pane();

    let inner_split = adw::NavigationSplitView::new();
    inner_split.set_sidebar(Some(&list_page));
    inner_split.set_content(Some(&reading_page));
    inner_split.set_min_sidebar_width(300.0);
    inner_split.set_max_sidebar_width(460.0);
    inner_split.set_sidebar_width_fraction(0.32);

    let outer_split = adw::NavigationSplitView::new();
    outer_split.set_sidebar(Some(&sidebar_page));
    outer_split.set_content(Some(&adw::NavigationPage::new(&inner_split, "Mail")));
    outer_split.set_min_sidebar_width(200.0);
    outer_split.set_max_sidebar_width(300.0);
    outer_split.set_sidebar_width_fraction(0.18);

    let (status_bar, status, unread_total) = mailbox::build_status_bar();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&outer_split));
    toolbar.add_bottom_bar(&status_bar);

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&toolbar));
    window.set_content(Some(&toasts));

    let ui: Ui = Rc::new(Widgets {
        window: window.clone(),
        toasts,
        search: search.clone(),
        compose_button: compose_button.clone(),
        sidebar,
        list_title,
        list_count,
        message_store: store,
        message_selection: selection,
        message_placeholder: placeholder,
        reading,
        status,
        unread_total,
    });

    // Search filters the list without re-querying the database.
    {
        let state = state.clone();
        let ui = ui.clone();
        search.connect_search_changed(move |entry| {
            state.borrow_mut().search = entry.text().to_string();
            mailbox::rebuild_message_list(&state, &ui);
        });
    }

    // Single-click activation, so a click opens the message the way the egui
    // rows did.
    {
        let state = state.clone();
        let ui = ui.clone();
        list_view.connect_activate(move |view, position| {
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

    for button in [&compose_button, &compose_sidebar] {
        let state = state.clone();
        let ui = ui.clone();
        button.connect_clicked(move |_| open_composer(&state, &ui));
    }

    {
        let state = state.clone();
        let ui = ui.clone();
        add_account_button.connect_clicked(move |_| open_setup(&state, &ui));
    }

    ui
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
