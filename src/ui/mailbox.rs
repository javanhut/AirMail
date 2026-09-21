//! The four mail panes — navigation, message list, reading pane and contact
//! card — plus the header around them.
//!
//! Each `build_*` runs once at startup and returns the widgets the state
//! writes into; each `rebuild_*`/`update_*` pushes current state into those
//! widgets. Nothing here reads the database directly except `open_message`
//! and the contact card.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::models::MessageSummary;
use crate::ui::message_object::MessageObject;
use crate::ui::{AppState, Filter, Ui, View, contact, theme};

/// Widgets in the reading pane that change per message.
pub struct ReadingPane {
    pub stack: gtk::Stack,
    pub toolbar: gtk::Box,
    pub subject: gtk::Label,
    pub avatar_slot: gtk::Box,
    pub sender: gtk::Label,
    pub address: gtk::Label,
    pub to: gtk::Label,
    pub date: gtk::Label,
    pub star: gtk::Button,
    pub unread_button: gtk::Button,
    pub body: gtk::TextView,
}

/// Header: the brand block, the search well and the two actions on the right.
pub struct Header {
    pub bar: adw::HeaderBar,
    pub search: gtk::Entry,
    pub compose: gtk::Button,
    pub menu: gtk::MenuButton,
}

/// The title, search field and actions across the top of the window.
pub fn build_header() -> Header {
    let bar = adw::HeaderBar::new();
    bar.set_show_title(false);

    // Brand block: the paper plane, the name and the tagline under it.
    let brand = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    brand.set_margin_start(4);
    let mark = gtk::Image::from_icon_name("mail-send-symbolic");
    mark.set_pixel_size(26);
    mark.add_css_class("accent");
    brand.append(&mark);

    let words = gtk::Box::new(gtk::Orientation::Vertical, 0);
    words.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(Some("AirMail"));
    name.add_css_class("title");
    name.set_xalign(0.0);
    let tagline = gtk::Label::new(Some("All your inboxes. One place."));
    tagline.add_css_class("tiny");
    tagline.add_css_class("faint");
    tagline.set_xalign(0.0);
    words.append(&name);
    words.append(&tagline);
    brand.append(&words);
    bar.pack_start(&brand);

    // The search well: a rounded box holding the icon, the entry and the
    // shortcut hint, so the hint sits inside the field the way it does in a
    // browser's omnibox. A bare GtkSearchEntry has nowhere to put it.
    let well = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    well.add_css_class("searchbox");
    well.set_hexpand(true);
    well.set_valign(gtk::Align::Center);
    let glass = gtk::Image::from_icon_name("system-search-symbolic");
    glass.set_pixel_size(16);
    well.append(&glass);

    let search = gtk::Entry::new();
    search.set_placeholder_text(Some("Search mail, contacts, files…"));
    search.set_has_frame(false);
    search.set_hexpand(true);
    search.set_width_request(420);
    well.append(&search);
    well.append(&theme::kbd("Ctrl K"));
    bar.set_title_widget(Some(&well));

    let compose = theme::icon_button("document-edit-symbolic", "Write a new message (Ctrl+N)");
    let menu = gtk::MenuButton::new();
    menu.set_icon_name("view-more-symbolic");
    menu.add_css_class("icon");
    menu.set_tooltip_text(Some("More"));
    bar.pack_end(&menu);
    bar.pack_end(&compose);

    Header {
        bar,
        search,
        compose,
        menu,
    }
}

/// The navigation sidebar. The returned box is the part that gets rebuilt;
/// the compose and add-account buttons outlive every rebuild.
pub fn build_sidebar() -> (adw::NavigationPage, gtk::Box, gtk::Button) {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 10);
    outer.add_css_class("panel");
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);
    outer.set_margin_start(10);
    outer.set_margin_end(10);

    // Compose carries its shortcut the way the mock does: icon, label, hint.
    let compose_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let pencil = gtk::Image::from_icon_name("document-edit-symbolic");
    pencil.set_pixel_size(16);
    compose_content.append(&pencil);
    let compose_label = gtk::Label::new(Some("Compose"));
    compose_label.set_hexpand(true);
    compose_label.set_xalign(0.0);
    compose_content.append(&compose_label);
    compose_content.append(&theme::kbd("Ctrl N"));

    let compose = gtk::Button::builder().child(&compose_content).build();
    compose.add_css_class("primary");
    compose.set_hexpand(true);
    outer.append(&compose);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    outer.append(&scroller);

    let page = adw::NavigationPage::new(&outer, "Mailboxes");
    (page, list, compose)
}

/// The message list: the filter chips over a `GtkListView` backed by a list
/// model of `MessageObject`s.
pub struct MessageList {
    pub page: adw::NavigationPage,
    pub chips: Vec<(Filter, gtk::Button)>,
    pub sort: gtk::Button,
    pub store: gtk::gio::ListStore,
    pub selection: gtk::SingleSelection,
    pub placeholder: gtk::Stack,
    pub view: gtk::ListView,
}

pub fn build_message_list() -> MessageList {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 6);
    outer.add_css_class("panel-raised");
    outer.add_css_class("column-edge");
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);

    // Chip row: the four filters, then the sort order on the right.
    let chip_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    chip_row.set_margin_start(10);
    chip_row.set_margin_end(10);
    chip_row.set_margin_bottom(4);

    let chips: Vec<(Filter, gtk::Button)> = Filter::ALL
        .iter()
        .map(|filter| {
            let button = gtk::Button::with_label(filter.label());
            button.add_css_class("chip");
            chip_row.append(&button);
            (*filter, button)
        })
        .collect();

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    chip_row.append(&spacer);

    let sort_content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let sort_label = gtk::Label::new(Some("Newest"));
    sort_label.set_widget_name("sort-label");
    sort_content.append(&sort_label);
    let chevron = gtk::Image::from_icon_name("pan-down-symbolic");
    chevron.set_pixel_size(14);
    sort_content.append(&chevron);
    let sort = gtk::Button::builder().child(&sort_content).build();
    sort.add_css_class("chip");
    sort.set_tooltip_text(Some("Sort order"));
    chip_row.append(&sort);
    outer.append(&chip_row);

    let store = gtk::gio::ListStore::new::<MessageObject>();
    let selection = gtk::SingleSelection::builder()
        .model(&store)
        .autoselect(false)
        .can_unselect(true)
        .build();

    let factory = gtk::SignalListItemFactory::new();
    // Rows are rebuilt on bind rather than set up once and mutated. Only the
    // handful of rows on screen exist at a time, and a fresh row cannot show
    // a previous message's avatar colour or star by accident.
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(object) = item.item().and_downcast::<MessageObject>() else {
            return;
        };
        item.set_child(Some(&message_row(&object.summary())));
    });

    let view = gtk::ListView::new(Some(selection.clone()), Some(factory));
    view.set_single_click_activate(true);
    view.add_css_class("messages");
    view.set_margin_start(6);
    view.set_margin_end(6);

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&view)
        .build();

    let empty = gtk::Label::new(Some("Nothing here yet."));
    empty.add_css_class("faint");
    empty.set_valign(gtk::Align::Center);

    let placeholder = gtk::Stack::new();
    placeholder.add_named(&scroller, Some("list"));
    placeholder.add_named(&empty, Some("empty"));
    placeholder.set_vexpand(true);
    outer.append(&placeholder);

    let page = adw::NavigationPage::new(&outer, "Messages");
    MessageList {
        page,
        chips,
        sort,
        store,
        selection,
        placeholder,
        view,
    }
}

/// One message in the list: avatar, sender and time, subject, preview, and
/// the attachment and star marks down the right.
fn message_row(summary: &MessageSummary) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("message-row");
    row.set_margin_top(2);
    row.set_margin_bottom(2);
    if !summary.seen {
        row.add_css_class("unread");
    }

    let sender = display_sender(&summary.from);
    let avatar = theme::avatar(&sender, 36);
    avatar.set_valign(gtk::Align::Start);
    row.append(&avatar);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let from = gtk::Label::new(Some(&sender));
    from.add_css_class("sender");
    from.set_xalign(0.0);
    from.set_hexpand(true);
    from.set_ellipsize(gtk::pango::EllipsizeMode::End);
    top.append(&from);

    let date = gtk::Label::new(Some(&summary.date.map(format_date).unwrap_or_default()));
    date.add_css_class("small");
    date.add_css_class("faint");
    date.set_valign(gtk::Align::Center);
    top.append(&date);
    text.append(&top);

    let subject = gtk::Label::new(Some(subject_or_placeholder(&summary.subject)));
    subject.add_css_class("row-subject");
    subject.set_xalign(0.0);
    subject.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&subject);

    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let preview = gtk::Label::new(Some(&preview_or_folder(summary)));
    preview.add_css_class("small");
    preview.add_css_class("faint");
    preview.set_xalign(0.0);
    preview.set_hexpand(true);
    preview.set_ellipsize(gtk::pango::EllipsizeMode::End);
    bottom.append(&preview);

    if summary.has_attachments {
        let clip = gtk::Image::from_icon_name("mail-attachment-symbolic");
        clip.set_pixel_size(14);
        clip.add_css_class("faint");
        bottom.append(&clip);
    }
    bottom.append(&star_icon(summary.flagged));
    text.append(&bottom);

    row.append(&text);
    row.upcast()
}

/// The star as it appears in a list row — an image, not a button: the row is
/// single-click-activate, and a button here would eat the click that opens
/// the message. Starring is done from the reading pane's toolbar.
fn star_icon(on: bool) -> gtk::Image {
    let star = gtk::Image::from_icon_name(if on {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    });
    star.set_pixel_size(14);
    star.add_css_class(if on { "star-on" } else { "faint" });
    star
}

pub fn build_reading_pane() -> (gtk::Box, ReadingPane) {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.add_css_class("bg-deep");
    outer.add_css_class("column-edge");

    // The action row sits above both states so the pane does not jump when a
    // message opens; its buttons are simply insensitive with nothing open.
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    toolbar.add_css_class("toolbar-row");
    toolbar.set_margin_start(14);
    toolbar.set_margin_end(14);
    toolbar.set_margin_top(8);
    toolbar.set_margin_bottom(8);

    let archive = theme::icon_button("folder-download-symbolic", "Archive");
    let delete = theme::icon_button("user-trash-symbolic", "Delete");
    let unread_button = theme::icon_button("mail-unread-symbolic", "Mark as unread");
    let star = theme::icon_button("non-starred-symbolic", "Star this message");
    star.add_css_class("star");
    let snooze = theme::icon_button("alarm-symbolic", "Snooze");
    let more = theme::icon_button("view-more-symbolic", "Reply, forward and more");

    // Archiving, deleting and snoozing all mean writing to the server, and
    // AirMail only ever reads from IMAP so far. They are shown so the row
    // keeps its shape, and held insensitive so they cannot lie.
    for (button, why) in [
        (&archive, "Archiving isn't available yet — AirMail doesn't move messages on the server"),
        (&delete, "Deleting isn't available yet — AirMail doesn't move messages on the server"),
        (&snooze, "Snoozing isn't available yet"),
    ] {
        button.set_sensitive(false);
        button.set_tooltip_text(Some(why));
    }

    for button in [&archive, &delete, &unread_button, &star, &snooze, &more] {
        toolbar.append(button);
    }
    outer.append(&toolbar);

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
    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.set_margin_start(22);
    content.set_margin_end(22);
    content.set_margin_top(14);
    content.set_margin_bottom(18);

    let subject = gtk::Label::new(None);
    subject.add_css_class("subject");
    subject.set_xalign(0.0);
    subject.set_wrap(true);
    content.append(&subject);

    let from_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let avatar_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    avatar_slot.set_valign(gtk::Align::Center);
    from_row.append(&avatar_slot);

    let who = gtk::Box::new(gtk::Orientation::Vertical, 2);
    who.set_hexpand(true);
    who.set_valign(gtk::Align::Center);

    let name_line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let sender = gtk::Label::new(None);
    sender.set_xalign(0.0);
    sender.add_css_class("sender");
    let address = gtk::Label::new(None);
    address.set_xalign(0.0);
    address.add_css_class("small");
    address.add_css_class("faint");
    address.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name_line.append(&sender);
    name_line.append(&address);
    who.append(&name_line);

    let to = gtk::Label::new(None);
    to.set_xalign(0.0);
    to.add_css_class("small");
    to.add_css_class("muted");
    to.set_ellipsize(gtk::pango::EllipsizeMode::End);
    who.append(&to);
    from_row.append(&who);

    let date = gtk::Label::new(None);
    date.add_css_class("small");
    date.add_css_class("muted");
    date.set_valign(gtk::Align::Center);
    from_row.append(&date);
    content.append(&from_row);

    let body = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(16)
        .right_margin(16)
        .top_margin(16)
        .bottom_margin(16)
        .build();
    body.add_css_class("reading");

    let card = theme::card();
    card.set_margin_top(0);
    card.set_margin_start(0);
    card.set_margin_end(0);
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

    (
        outer,
        ReadingPane {
            stack,
            toolbar,
            subject,
            avatar_slot,
            sender,
            address,
            to,
            date,
            star,
            unread_button,
            body,
        },
    )
}

/// The status line: whatever happened last, plus the unread total. Hidden
/// while nothing has happened, which is how the window matches the design at
/// rest and still has somewhere to say "Sending…".
pub fn build_status_bar() -> (gtk::Box, gtk::Label, gtk::Label) {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.add_css_class("statusbar");
    bar.set_margin_top(5);
    bar.set_margin_bottom(5);
    bar.set_margin_start(14);
    bar.set_margin_end(14);

    let status = gtk::Label::new(None);
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

    let snapshot = state.borrow().sidebar_snapshot();

    // The smart mailboxes, in the order the design lists them.
    for (smart, count) in &snapshot.smart {
        let selected = snapshot.view == View::Smart(*smart);
        let row = nav_row(smart.label(), Some(smart.icon()), selected, *count, 0);
        ui.sidebar.append(&row);
        let state = state.clone();
        let ui = ui.clone();
        let smart = *smart;
        row.connect_clicked(move |_| select_view(&state, &ui, View::Smart(smart)));
    }

    // "More" opens the raw folder tree underneath, which is the only place
    // folders that map to no smart mailbox can be reached.
    let more = nav_row("More", Some("view-more-symbolic"), false, 0, 0);
    {
        let state = state.clone();
        let ui = ui.clone();
        more.connect_clicked(move |_| {
            {
                let mut state = state.borrow_mut();
                state.show_all_folders = !state.show_all_folders;
            }
            rebuild_sidebar(&state, &ui);
        });
    }
    ui.sidebar.append(&more);

    if snapshot.show_all_folders {
        for account in &snapshot.accounts {
            let heading = theme::section_label(strip_brackets(&account.email));
            heading.set_margin_top(8);
            heading.set_margin_start(14);
            heading.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            ui.sidebar.append(&heading);
            for folder in &account.folders {
                let selected = matches!(&snapshot.view, View::Folder(f, _) if *f == folder.id);
                let row = nav_row(
                    &folder.name,
                    Some("folder-symbolic"),
                    selected,
                    folder.unread,
                    14,
                );
                ui.sidebar.append(&row);
                let target = View::Folder(folder.id, folder.name.clone());
                let state = state.clone();
                let ui = ui.clone();
                row.connect_clicked(move |_| select_view(&state, &ui, target.clone()));
            }
        }
    }

    // Accounts.
    let accounts_heading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    accounts_heading.set_margin_top(14);
    accounts_heading.set_margin_start(8);
    let label = theme::section_label("Accounts");
    label.set_hexpand(true);
    accounts_heading.append(&label);
    let add_account = theme::icon_button("list-add-symbolic", "Add an account");
    {
        let state = state.clone();
        let ui = ui.clone();
        add_account.connect_clicked(move |_| crate::ui::open_setup(&state, &ui));
    }
    accounts_heading.append(&add_account);
    ui.sidebar.append(&accounts_heading);

    if snapshot.accounts.is_empty() {
        let empty = gtk::Label::new(Some("No accounts yet."));
        empty.add_css_class("small");
        empty.add_css_class("faint");
        empty.set_xalign(0.0);
        empty.set_margin_start(8);
        empty.set_margin_top(6);
        ui.sidebar.append(&empty);
    }

    for account in &snapshot.accounts {
        let selected = matches!(&snapshot.view, View::Account(a) if *a == account.email);
        let row = account_row(&account.email, account.unread, selected);
        ui.sidebar.append(&row);
        let state = state.clone();
        let ui = ui.clone();
        let email = account.email.clone();
        row.connect_clicked(move |_| {
            select_view(&state, &ui, View::Account(email.clone()));
        });
    }

    // Labels — the folders that are nobody's inbox, drafts or trash. On Gmail
    // these are literally the user's labels; elsewhere they are custom
    // folders, which is the same idea.
    if !snapshot.labels.is_empty() {
        let heading = theme::section_label("Labels");
        heading.set_margin_top(14);
        heading.set_margin_start(8);
        ui.sidebar.append(&heading);

        for (id, name) in &snapshot.labels {
            let selected = matches!(&snapshot.view, View::Folder(f, _) if f == id);
            let row = label_row(name, selected);
            ui.sidebar.append(&row);
            let target = View::Folder(*id, name.clone());
            let state = state.clone();
            let ui = ui.clone();
            row.connect_clicked(move |_| select_view(&state, &ui, target.clone()));
        }
    }
}

/// One clickable sidebar line: icon, name, optional count.
fn nav_row(
    label: &str,
    icon: Option<&str>,
    selected: bool,
    count: i64,
    indent: i32,
) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.set_margin_start(indent);

    if let Some(icon) = icon {
        let image = gtk::Image::from_icon_name(icon);
        image.set_pixel_size(16);
        content.append(&image);
    }

    let name = gtk::Label::new(Some(label));
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&name);

    if count > 0 {
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

/// An account in the sidebar: avatar, the name it goes by, the address under
/// it, and its unread badge.
fn account_row(email: &str, unread: i64, selected: bool) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.append(&theme::avatar(email, 28));

    let words = gtk::Box::new(gtk::Orientation::Vertical, 0);
    words.set_hexpand(true);
    words.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(Some(&account_name(email)));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let address = gtk::Label::new(Some(strip_brackets(email)));
    address.add_css_class("tiny");
    address.add_css_class("faint");
    address.set_xalign(0.0);
    address.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    words.append(&name);
    words.append(&address);
    content.append(&words);

    if unread > 0 {
        content.append(&theme::count_badge(unread, false));
    }

    let row = gtk::Button::builder().child(&content).build();
    row.add_css_class("flat");
    row.add_css_class("nav-row");
    row.set_tooltip_text(Some(email));
    if selected {
        row.add_css_class("selected");
    }
    row
}

/// A label in the sidebar: a coloured dot and the folder's leaf name.
fn label_row(name: &str, selected: bool) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.set_margin_start(2);
    let dot = theme::label_dot(name);
    dot.set_margin_start(3);
    dot.set_margin_end(3);
    content.append(&dot);

    let label = gtk::Label::new(Some(leaf_name(name)));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&label);

    let row = gtk::Button::builder().child(&content).build();
    row.add_css_class("flat");
    row.add_css_class("nav-row");
    row.set_tooltip_text(Some(name));
    if selected {
        row.add_css_class("selected");
    }
    row
}

pub fn select_view(state: &Rc<RefCell<AppState>>, ui: &Ui, view: View) {
    {
        let mut state = state.borrow_mut();
        state.view = view;
        state.detail = None;
        state.refresh_summaries();
    }
    crate::ui::refresh_all(state, ui);
}

/// Mark the chip matching the live filter, and say which way the list sorts.
pub fn update_chips(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    let (filter, newest_first) = {
        let state = state.borrow();
        (state.filter, state.newest_first)
    };
    for (chip_filter, button) in &ui.chips {
        if *chip_filter == filter {
            button.add_css_class("selected");
        } else {
            button.remove_css_class("selected");
        }
    }
    if let Some(label) = ui
        .sort_button
        .child()
        .and_downcast::<gtk::Box>()
        .and_then(|b| b.first_child())
        .and_downcast::<gtk::Label>()
    {
        label.set_text(if newest_first { "Newest" } else { "Oldest" });
    }
}

pub fn rebuild_message_list(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    update_chips(state, ui);

    let (rows, empty_text, selected_id) = {
        let state = state.borrow();
        let rows: Vec<MessageSummary> = state.visible_summaries().into_iter().cloned().collect();
        let selected = state.detail.as_ref().map(|d| d.summary.id);
        (rows, state.empty_text(), selected)
    };

    if rows.is_empty() {
        if let Some(label) = ui
            .message_placeholder
            .child_by_name("empty")
            .and_downcast::<gtk::Label>()
        {
            label.set_text(empty_text);
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
        for button in [&ui.reading.star, &ui.reading.unread_button] {
            button.set_sensitive(false);
        }
        ui.reading.star.remove_css_class("on");
        ui.reading.star.set_icon_name("non-starred-symbolic");
        contact::clear(ui);
        return;
    };
    for button in [&ui.reading.star, &ui.reading.unread_button] {
        button.set_sensitive(true);
    }

    ui.reading
        .subject
        .set_text(subject_or_placeholder(&detail.summary.subject));

    let sender = display_sender(&detail.summary.from);
    let address = sender_address(&detail.summary.from);
    ui.reading.sender.set_text(&sender);
    ui.reading.address.set_text(&if address.is_empty() || address == sender {
        String::new()
    } else {
        format!("<{address}>")
    });
    ui.reading.to.set_text(&format!("to {}", detail.to));
    ui.reading.date.set_text(
        &detail
            .summary
            .date
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%b %-d, %Y · %-I:%M %p")
                    .to_string()
            })
            .unwrap_or_default(),
    );

    let starred = detail.summary.flagged;
    ui.reading.star.set_icon_name(if starred {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    });
    ui.reading.star.set_tooltip_text(Some(if starred {
        "Remove the star"
    } else {
        "Star this message"
    }));
    if starred {
        ui.reading.star.add_css_class("on");
    } else {
        ui.reading.star.remove_css_class("on");
    }

    while let Some(child) = ui.reading.avatar_slot.first_child() {
        ui.reading.avatar_slot.remove(&child);
    }
    ui.reading.avatar_slot.append(&theme::avatar(&sender, 44));

    if detail.body_text.trim().is_empty() {
        ui.reading
            .body
            .buffer()
            .set_text("This message has no plain-text part, and HTML isn't rendered yet.");
    } else {
        ui.reading.body.buffer().set_text(&detail.body_text);
    }
    ui.reading.stack.set_visible_child_name("message");

    contact::update(state, ui, &detail);
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

/// Flip the star on the open message, in the local cache. Nothing is written
/// back to IMAP yet, so the next full resync of the folder is what makes it
/// stick on the server's side — see `sync::worker`.
pub fn toggle_star(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    let Some((id, flagged)) = state
        .borrow()
        .detail
        .as_ref()
        .map(|d| (d.summary.id, d.summary.flagged))
    else {
        return;
    };
    let result = {
        let mut state = state.borrow_mut();
        let result = state.db().set_flagged(id, !flagged);
        state.refresh_summaries_keep_selection();
        result
    };
    match result {
        Ok(()) => crate::ui::refresh_all(state, ui),
        Err(e) => crate::ui::set_status(state, ui, format!("Could not star it: {e:#}"), true),
    }
}

/// Put the open message back to unread and close it, the way every other
/// client does when you press the envelope.
pub fn mark_unread(state: &Rc<RefCell<AppState>>, ui: &Ui) {
    let Some(id) = state.borrow().detail.as_ref().map(|d| d.summary.id) else {
        return;
    };
    let result = {
        let mut state = state.borrow_mut();
        let result = state.db().set_seen(id, false);
        state.detail = None;
        state.refresh_summaries();
        state.refresh_folders();
        result
    };
    match result {
        Ok(()) => crate::ui::refresh_all(state, ui),
        Err(e) => crate::ui::set_status(state, ui, format!("Could not mark it unread: {e:#}"), true),
    }
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

/// The bare address out of a From header, for the contact pane and for
/// matching one correspondent's mail against another's.
pub fn sender_address(from: &str) -> String {
    let from = from.trim();
    if let Some((_, rest)) = from.split_once('<') {
        return rest.trim_end_matches('>').trim().to_string();
    }
    from.trim_matches(['<', '>']).to_string()
}

/// Drop the angle brackets some servers keep around an address.
pub fn strip_brackets(email: &str) -> &str {
    email.trim().trim_matches(['<', '>'])
}

/// `[Gmail]/Sent Mail` is listed as `Sent Mail`: the hierarchy is noise in a
/// sidebar that is only ever one level deep.
pub fn leaf_name(folder: &str) -> &str {
    folder
        .rsplit(['/', '.'])
        .next()
        .filter(|leaf| !leaf.is_empty())
        .unwrap_or(folder)
}

/// What an account is called when it has no display name: the part before the
/// `@`, capitalised, which is what most people would have typed anyway.
pub fn account_name(email: &str) -> String {
    let local = strip_brackets(email).split('@').next().unwrap_or(email);
    let cleaned = local.replace(['.', '_', '-'], " ");
    let mut out = String::with_capacity(cleaned.len());
    for word in cleaned.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        strip_brackets(email).to_string()
    } else {
        out
    }
}

pub fn subject_or_placeholder(subject: &str) -> &str {
    if subject.trim().is_empty() {
        "(no subject)"
    } else {
        subject
    }
}

/// The row's third line: the body's opening words, or the folder it lives in
/// when there is no text part to quote.
fn preview_or_folder(summary: &MessageSummary) -> String {
    let preview = summary.preview.trim();
    if preview.is_empty() {
        leaf_name(&summary.folder_name).to_string()
    } else {
        preview.to_string()
    }
}

/// Today shows a time, yesterday says so, this year a date, older the year
/// too — the same shorthand every mail client uses.
pub fn format_date(date: chrono::DateTime<chrono::Utc>) -> String {
    let local = date.with_timezone(&chrono::Local);
    let now = chrono::Local::now();
    let today = now.date_naive();
    let elapsed = now.signed_duration_since(local);
    if local.date_naive() == today {
        local.format("%-I:%M %p").to_string()
    } else if today.signed_duration_since(local.date_naive()).num_days() == 1 {
        "Yesterday".to_string()
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
    fn the_address_comes_out_whatever_the_name_is() {
        assert_eq!(sender_address("\"Ada Lovelace\" <ada@x.y>"), "ada@x.y");
        assert_eq!(sender_address("ada@x.y"), "ada@x.y");
        assert_eq!(sender_address("<ada@x.y>"), "ada@x.y");
    }

    #[test]
    fn brackets_come_off_addresses() {
        assert_eq!(strip_brackets(" <ada@x.y> "), "ada@x.y");
        assert_eq!(strip_brackets("ada@x.y"), "ada@x.y");
    }

    #[test]
    fn folders_are_listed_by_their_leaf() {
        assert_eq!(leaf_name("[Gmail]/Sent Mail"), "Sent Mail");
        assert_eq!(leaf_name("INBOX.Travel"), "Travel");
        assert_eq!(leaf_name("Archive"), "Archive");
    }

    #[test]
    fn an_account_without_a_name_is_called_after_its_address() {
        assert_eq!(account_name("ada.lovelace@x.y"), "Ada Lovelace");
        assert_eq!(account_name("<work@x.y>"), "Work");
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
