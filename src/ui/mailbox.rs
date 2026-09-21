//! The three mail panes — accounts, message list, reading pane — plus the
//! header and status bars around them.
//!
//! Each `build_*` runs once at startup and returns the widgets the state
//! writes into; each `rebuild_*`/`update_*` pushes current state into those
//! widgets. Nothing here reads the database directly except `open_message`.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::models::MessageSummary;
use crate::ui::message_object::MessageObject;
use crate::ui::{AppState, Ui, View, theme};

/// Widgets in the reading pane that change per message.
pub struct ReadingPane {
    pub stack: gtk::Stack,
    pub subject: gtk::Label,
    pub avatar_slot: gtk::Box,
    pub sender: gtk::Label,
    pub to: gtk::Label,
    pub date: gtk::Label,
    pub body: gtk::TextView,
}

/// Title, search and the compose button, across the top of the window.
pub fn build_header() -> (adw::HeaderBar, gtk::SearchEntry, gtk::Button) {
    let header = adw::HeaderBar::new();

    let title = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let name = gtk::Label::new(Some("AirMail"));
    name.add_css_class("title");
    let tagline = gtk::Label::new(Some("All your inboxes. One place."));
    tagline.add_css_class("small");
    tagline.add_css_class("faint");
    title.append(&name);
    title.append(&tagline);
    header.pack_start(&title);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search mail by sender, subject or folder…"));
    search.set_width_request(360);
    search.set_hexpand(true);
    header.set_title_widget(Some(&search));

    let compose = theme::primary_button("Compose");
    compose.set_tooltip_text(Some("Write a new message"));
    header.pack_end(&compose);

    (header, search, compose)
}

/// The accounts sidebar. The returned box is the part that gets rebuilt; the
/// compose and add-account buttons outlive every rebuild.
pub fn build_sidebar() -> (adw::NavigationPage, gtk::Box, gtk::Button, gtk::Button) {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 10);
    outer.add_css_class("panel");
    theme::set_margins(&outer, 10);

    let compose = theme::primary_button("＋  Compose");
    compose.set_hexpand(true);
    outer.append(&compose);

    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let label = gtk::Label::new(Some("ACCOUNTS"));
    label.add_css_class("section-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    let add_account = gtk::Button::from_icon_name("list-add-symbolic");
    add_account.add_css_class("flat");
    add_account.set_tooltip_text(Some("Add an account"));
    heading.append(&label);
    heading.append(&add_account);
    outer.append(&heading);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    outer.append(&scroller);

    let page = adw::NavigationPage::new(&outer, "Accounts");
    (page, list, add_account, compose)
}

/// The message list: a title line over a `GtkListView` backed by a list model
/// of `MessageObject`s.
#[allow(clippy::type_complexity)]
pub fn build_message_list() -> (
    adw::NavigationPage,
    gtk::Label,
    gtk::Label,
    gtk::gio::ListStore,
    gtk::SingleSelection,
    gtk::Stack,
    gtk::ListView,
) {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 6);
    outer.add_css_class("panel");
    theme::set_margins(&outer, 8);

    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("All accounts"));
    title.add_css_class("heading-sm");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let count = gtk::Label::new(None);
    count.add_css_class("small");
    count.add_css_class("faint");
    heading.append(&title);
    heading.append(&count);
    outer.append(&heading);

    let store = gtk::gio::ListStore::new::<MessageObject>();
    let selection = gtk::SingleSelection::builder()
        .model(&store)
        .autoselect(false)
        .can_unselect(true)
        .build();

    let factory = gtk::SignalListItemFactory::new();
    // Rows are rebuilt on bind rather than set up once and mutated. Only the
    // handful of rows on screen exist at a time, and a fresh row cannot show
    // a previous message's avatar colour or unread dot by accident.
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(object) = item.item().and_downcast::<MessageObject>() else {
            return;
        };
        item.set_child(Some(&message_row(&object.summary())));
    });

    let list_view = gtk::ListView::new(Some(selection.clone()), Some(factory));
    list_view.set_single_click_activate(true);
    list_view.add_css_class("navigation-sidebar");

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list_view)
        .build();

    let empty = gtk::Label::new(Some("Nothing here yet."));
    empty.add_css_class("faint");
    empty.set_valign(gtk::Align::Center);

    let stack = gtk::Stack::new();
    stack.add_named(&scroller, Some("list"));
    stack.add_named(&empty, Some("empty"));
    stack.set_vexpand(true);
    outer.append(&stack);

    let page = adw::NavigationPage::new(&outer, "Messages");
    (page, title, count, store, selection, stack, list_view)
}

/// One message in the list: avatar, sender, subject, date, unread dot.
fn message_row(summary: &MessageSummary) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("message-row");
    row.set_height_request(62);
    theme::set_margins(&row, 4);
    if !summary.seen {
        row.add_css_class("unread");
    }

    let sender = display_sender(&summary.from);
    row.append(&theme::avatar(&sender, 32));

    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);

    let from = gtk::Label::new(Some(&sender));
    from.add_css_class("sender");
    from.set_xalign(0.0);
    from.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let subject_text = if summary.subject.trim().is_empty() {
        "(no subject)"
    } else {
        &summary.subject
    };
    let subject = gtk::Label::new(Some(subject_text));
    subject.add_css_class("subject");
    subject.set_xalign(0.0);
    subject.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let meta_text = if summary.has_attachments {
        format!("{}  ·  has attachments", summary.folder_name)
    } else {
        summary.folder_name.clone()
    };
    let meta = gtk::Label::new(Some(&meta_text));
    meta.add_css_class("small");
    meta.add_css_class("faint");
    meta.set_xalign(0.0);
    meta.set_ellipsize(gtk::pango::EllipsizeMode::End);

    text.append(&from);
    text.append(&subject);
    text.append(&meta);
    row.append(&text);

    let trailing = gtk::Box::new(gtk::Orientation::Vertical, 6);
    trailing.set_valign(gtk::Align::Fill);
    let date = gtk::Label::new(Some(&summary.date.map(format_date).unwrap_or_default()));
    date.add_css_class("small");
    date.add_css_class("faint");
    date.set_halign(gtk::Align::End);
    trailing.append(&date);

    if !summary.seen {
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("unread-dot");
        dot.set_halign(gtk::Align::End);
        dot.set_valign(gtk::Align::End);
        dot.set_vexpand(true);
        trailing.append(&dot);
    }
    row.append(&trailing);

    row.upcast()
}

pub fn build_reading_pane() -> (adw::NavigationPage, ReadingPane) {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.add_css_class("bg-deep");

    // Nothing open yet.
    let empty = gtk::Box::new(gtk::Orientation::Vertical, 4);
    empty.set_valign(gtk::Align::Center);
    let empty_title = gtk::Label::new(Some("Select a message"));
    empty_title.add_css_class("heading-sm");
    empty_title.add_css_class("muted");
    let empty_hint = gtk::Label::new(Some("Nothing is open right now."));
    empty_hint.add_css_class("small");
    empty_hint.add_css_class("faint");
    empty.append(&empty_title);
    empty.append(&empty_hint);

    // An open message.
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    theme::set_margins(&content, 18);

    let subject = gtk::Label::new(None);
    subject.add_css_class("subject");
    subject.set_xalign(0.0);
    subject.set_wrap(true);
    content.append(&subject);

    let from_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let avatar_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    from_row.append(&avatar_slot);

    let who = gtk::Box::new(gtk::Orientation::Vertical, 1);
    who.set_hexpand(true);
    let sender = gtk::Label::new(None);
    sender.set_xalign(0.0);
    sender.add_css_class("sender");
    let to = gtk::Label::new(None);
    to.set_xalign(0.0);
    to.add_css_class("small");
    to.add_css_class("muted");
    to.set_ellipsize(gtk::pango::EllipsizeMode::End);
    who.append(&sender);
    who.append(&to);
    from_row.append(&who);

    let date = gtk::Label::new(None);
    date.add_css_class("small");
    date.add_css_class("muted");
    date.set_valign(gtk::Align::Start);
    from_row.append(&date);
    content.append(&from_row);

    let body = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(14)
        .right_margin(14)
        .top_margin(14)
        .bottom_margin(14)
        .build();
    body.add_css_class("reading");

    let card = theme::card();
    card.append(&body);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&card)
        .build();
    content.append(&scroller);

    let stack = gtk::Stack::new();
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&content, Some("message"));
    stack.set_vexpand(true);
    outer.append(&stack);

    let page = adw::NavigationPage::new(&outer, "Message");
    (
        page,
        ReadingPane {
            stack,
            subject,
            avatar_slot,
            sender,
            to,
            date,
            body,
        },
    )
}

/// The status line: whatever happened last, plus the unread total.
pub fn build_status_bar() -> (gtk::Box, gtk::Label, gtk::Label) {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.add_css_class("statusbar");
    bar.set_margin_top(6);
    bar.set_margin_bottom(6);
    bar.set_margin_start(14);
    bar.set_margin_end(14);

    let status = gtk::Label::new(Some("Ready"));
    status.add_css_class("small");
    status.add_css_class("muted");
    status.set_xalign(0.0);
    status.set_hexpand(true);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let unread = gtk::Label::new(None);
    unread.add_css_class("small");
    unread.add_css_class("faint");

    bar.append(&status);
    bar.append(&unread);
    (bar, status, unread)
}

// ---------------------------------------------------------------------------
// State → widgets
// ---------------------------------------------------------------------------

pub fn rebuild_sidebar(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    while let Some(child) = ui.sidebar.first_child() {
        ui.sidebar.remove(&child);
    }

    let (view, cache, unified_unread, per_account, per_folder) = {
        let state = state.borrow();
        let unified: i64 = state.unread.values().sum();
        let per_account: Vec<i64> = state
            .folders_cache
            .iter()
            .map(|(email, _)| state.unread_for_account(email))
            .collect();
        let per_folder: Vec<Vec<i64>> = state
            .folders_cache
            .iter()
            .map(|(_, folders)| {
                folders
                    .iter()
                    .map(|f| state.unread.get(&f.id).copied().unwrap_or(0))
                    .collect()
            })
            .collect();
        (
            state.view.clone(),
            state.folders_cache.clone(),
            unified,
            per_account,
            per_folder,
        )
    };

    let unified = nav_row(
        "Unified inbox",
        matches!(view, View::Unified),
        Some(unified_unread),
        0,
    );
    {
        let state = state.clone();
        let ui = ui.clone();
        unified.connect_clicked(move |_| select_view(&state, &ui, View::Unified));
    }
    ui.sidebar.append(&unified);

    if cache.is_empty() {
        let empty = gtk::Label::new(Some("No accounts yet."));
        empty.add_css_class("small");
        empty.add_css_class("faint");
        empty.set_xalign(0.0);
        empty.set_margin_top(8);
        ui.sidebar.append(&empty);
        return;
    }

    for (account_index, (email, folders)) in cache.iter().enumerate() {
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.set_margin_top(10);
        header.append(&theme::avatar(email, 22));

        let label = gtk::Label::new(Some(strip_brackets(email)));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.set_tooltip_text(Some(email));
        header.append(&label);

        let unread = per_account.get(account_index).copied().unwrap_or(0);
        if unread > 0 {
            header.append(&theme::count_badge(unread, false));
        }

        let remove = gtk::Button::from_icon_name("window-close-symbolic");
        remove.add_css_class("flat");
        remove.set_tooltip_text(Some(&format!("Remove {email}")));
        {
            let state = state.clone();
            let ui = ui.clone();
            let email = email.clone();
            remove.connect_clicked(move |_| crate::ui::confirm_removal(&state, &ui, &email));
        }
        header.append(&remove);
        ui.sidebar.append(&header);

        for (folder_index, folder) in folders.iter().enumerate() {
            let selected = matches!(&view, View::Folder(id, _) if *id == folder.id);
            let unread = per_folder
                .get(account_index)
                .and_then(|counts| counts.get(folder_index))
                .copied()
                .unwrap_or(0);
            let row = nav_row(&folder.name, selected, (unread > 0).then_some(unread), 14);
            ui.sidebar.append(&row);
            let target = View::Folder(folder.id, folder.name.clone());
            let state = state.clone();
            let ui = ui.clone();
            row.connect_clicked(move |_| select_view(&state, &ui, target.clone()));
        }
    }
}

/// One clickable sidebar line: rounded highlight, name, optional count.
fn nav_row(label: &str, selected: bool, count: Option<i64>, indent: i32) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_margin_start(indent);

    let name = gtk::Label::new(Some(label));
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&name);

    if let Some(count) = count.filter(|c| *c > 0) {
        let badge = gtk::Label::new(Some(&count.to_string()));
        badge.add_css_class("small");
        badge.add_css_class(if selected { "accent" } else { "faint" });
        content.append(&badge);
    }

    let row = gtk::Button::builder().child(&content).build();
    row.add_css_class("flat");
    row.add_css_class("nav-row");
    if selected {
        row.add_css_class("selected");
    }
    row
}

fn select_view(state: &Rc<RefCell<AppState>>, ui: &Ui, view: View) {
    {
        let mut state = state.borrow_mut();
        state.view = view;
        state.detail = None;
        state.refresh_summaries();
    }
    crate::ui::refresh_all(state, ui);
}

pub fn rebuild_message_list(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    let (rows, title, searching, selected_id) = {
        let state = state.borrow();
        let title = match &state.view {
            View::Unified => "All accounts".to_string(),
            View::Folder(_, name) => name.clone(),
        };
        let rows: Vec<MessageSummary> = state.visible_summaries().into_iter().cloned().collect();
        let selected = state.detail.as_ref().map(|d| d.summary.id);
        (rows, title, !state.search.trim().is_empty(), selected)
    };

    ui.list_title.set_text(&title);
    ui.list_count.set_text(&match rows.len() {
        1 => "1 message".to_string(),
        n => format!("{n} messages"),
    });

    if rows.is_empty() {
        if let Some(label) = ui
            .message_placeholder
            .child_by_name("empty")
            .and_downcast::<gtk::Label>()
        {
            label.set_text(if searching {
                "No messages match that search."
            } else {
                "Nothing here yet."
            });
        }
        ui.message_store.remove_all();
        ui.message_placeholder.set_visible_child_name("empty");
        return;
    }
    ui.message_placeholder.set_visible_child_name("list");

    let objects: Vec<MessageObject> = rows.into_iter().map(MessageObject::new).collect();
    let position =
        selected_id.and_then(|id| objects.iter().position(|o| o.id() == id).map(|p| p as u32));
    // `splice` swaps the whole model in one signal, so the view rebinds once
    // rather than once per row.
    let removed = ui.message_store.n_items();
    ui.message_store.splice(0, removed, &objects);

    match position {
        Some(position) => ui.message_selection.set_selected(position),
        None => ui
            .message_selection
            .set_selected(gtk::INVALID_LIST_POSITION),
    }
}

pub fn update_reading_pane(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    let detail = state.borrow().detail.clone();
    let Some(detail) = detail else {
        ui.reading.stack.set_visible_child_name("empty");
        return;
    };

    let subject = if detail.summary.subject.trim().is_empty() {
        "(no subject)".to_string()
    } else {
        detail.summary.subject.clone()
    };
    ui.reading.subject.set_text(&subject);

    let sender = display_sender(&detail.summary.from);
    ui.reading.sender.set_text(&sender);
    ui.reading.to.set_text(&format!("to {}", detail.to));
    ui.reading.date.set_text(
        &detail
            .summary
            .date
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%b %-d, %Y at %H:%M")
                    .to_string()
            })
            .unwrap_or_default(),
    );

    while let Some(child) = ui.reading.avatar_slot.first_child() {
        ui.reading.avatar_slot.remove(&child);
    }
    ui.reading.avatar_slot.append(&theme::avatar(&sender, 34));

    if detail.body_text.trim().is_empty() {
        ui.reading
            .body
            .buffer()
            .set_text("This message has no plain-text part, and HTML isn't rendered yet.");
    } else {
        ui.reading.body.buffer().set_text(&detail.body_text);
    }
    ui.reading.stack.set_visible_child_name("message");
}

pub fn open_message(state: &Rc<RefCell<AppState>>, ui: &Ui, id: i64) {
    let status = {
        let mut state = state.borrow_mut();
        match state.db().message_detail(id) {
            Ok(Some(detail)) => {
                state.detail = Some(detail);
                let mark = state.db().set_seen(id, true);
                state.refresh_summaries_keep_selection();
                state.refresh_folders();
                mark.err()
                    .map(|e| format!("Could not mark the message read: {e:#}"))
            }
            Ok(None) => Some("That message no longer exists".to_string()),
            Err(e) => Some(format!("Loading the message failed: {e:#}")),
        }
    };
    if let Some(status) = status {
        state.borrow_mut().status = status;
    }
    crate::ui::refresh_all(state, ui);
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// `"Ada Lovelace" <ada@example.com>` reads better as just `Ada Lovelace`.
pub fn display_sender(from: &str) -> String {
    let from = from.trim();
    if let Some((name, _)) = from.split_once('<') {
        let name = name.trim().trim_matches('"').trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    from.trim_matches(['<', '>']).to_string()
}

/// Drop the angle brackets some servers keep around an address.
pub fn strip_brackets(email: &str) -> &str {
    email.trim().trim_matches(['<', '>'])
}

/// Recent mail shows a time, this year a date, older the year too — the same
/// shorthand every mail client uses.
pub fn format_date(date: chrono::DateTime<chrono::Utc>) -> String {
    let local = date.with_timezone(&chrono::Local);
    let now = chrono::Local::now();
    let elapsed = now.signed_duration_since(local);
    if elapsed.num_hours() < 24 && local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else if elapsed.num_days() < 300 {
        local.format("%b %-d").to_string()
    } else {
        local.format("%b %-d, %Y").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_prefers_the_display_name() {
        assert_eq!(display_sender("\"Ada Lovelace\" <ada@x.y>"), "Ada Lovelace");
        assert_eq!(display_sender("Ada Lovelace <ada@x.y>"), "Ada Lovelace");
        assert_eq!(display_sender("<ada@x.y>"), "ada@x.y");
        assert_eq!(display_sender("  ada@x.y  "), "ada@x.y");
    }

    #[test]
    fn brackets_come_off_addresses() {
        assert_eq!(strip_brackets(" <ada@x.y> "), "ada@x.y");
        assert_eq!(strip_brackets("ada@x.y"), "ada@x.y");
    }

    #[test]
    fn dates_shorten_by_age() {
        let now = chrono::Utc::now();
        // Today: a time.
        assert!(format_date(now).contains(':'));
        // Last week: a day and month, no year.
        let week = now - chrono::Duration::days(7);
        let week = format_date(week);
        assert!(!week.contains(':'), "{week} should not carry a time");
        assert!(!week.contains(','), "{week} should not carry a year");
        // Two years back: the year too.
        let old = format_date(now - chrono::Duration::days(730));
        assert!(old.contains(','), "{old} should carry a year");
    }
}
