//! The contact card down the right-hand edge: who sent the open message,
//! what AirMail knows about them, and the rest of their mail.
//!
//! Everything in it is derived from the local database — there is no address
//! book and no network lookup — so the pane is empty exactly when no message
//! is open.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::models::MessageDetail;
use crate::ui::mailbox::{display_sender, format_date, leaf_name, sender_address};
use crate::ui::{AppState, Ui, View, theme};

/// How many of a correspondent's other messages the card lists.
const RECENT: usize = 6;

pub struct ContactPane {
    pub root: gtk::Box,
    pub stack: gtk::Stack,
    pub avatar_slot: gtk::Box,
    pub name: gtk::Label,
    pub address: gtk::Label,
    pub reply: gtk::Button,
    pub compose: gtk::Button,
    pub copy: gtk::Button,
    pub find: gtk::Button,
    pub about: gtk::Box,
    pub folders: gtk::Box,
    pub folders_heading: gtk::Label,
    pub recent_heading: gtk::Label,
    pub recent: gtk::Box,
}

pub fn build() -> ContactPane {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("panel");
    root.add_css_class("column-edge");
    root.set_width_request(250);

    let empty = gtk::Label::new(Some("No message open."));
    empty.add_css_class("small");
    empty.add_css_class("faint");
    empty.set_valign(gtk::Align::Center);
    empty.set_wrap(true);
    empty.set_justify(gtk::Justification::Center);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.set_margin_top(20);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);

    let avatar_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    avatar_slot.set_halign(gtk::Align::Start);
    content.append(&avatar_slot);

    let who = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let name = gtk::Label::new(None);
    name.add_css_class("heading-sm");
    name.set_xalign(0.0);
    name.set_wrap(true);
    let address = gtk::Label::new(None);
    address.add_css_class("small");
    address.add_css_class("faint");
    address.set_xalign(0.0);
    address.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    who.append(&name);
    who.append(&address);
    content.append(&who);

    // The row of square actions under the name.
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let reply = tile("mail-reply-sender-symbolic", "Reply to this message");
    let compose = tile("mail-message-new-symbolic", "Write to them");
    let copy = tile("edit-copy-symbolic", "Copy their address");
    let find = tile("system-search-symbolic", "Search their mail");
    for button in [&reply, &compose, &copy, &find] {
        actions.append(button);
    }
    content.append(&actions);

    let about_heading = theme::section_label("About");
    about_heading.set_margin_top(6);
    content.append(&about_heading);
    let about = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.append(&about);

    let folders_heading = theme::section_label("Folders");
    folders_heading.set_margin_top(6);
    content.append(&folders_heading);
    let folders = gtk::Box::new(gtk::Orientation::Vertical, 1);
    content.append(&folders);

    let recent_heading = theme::section_label("Recent");
    recent_heading.set_margin_top(6);
    content.append(&recent_heading);
    let recent = gtk::Box::new(gtk::Orientation::Vertical, 1);
    content.append(&recent);

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();

    let stack = gtk::Stack::new();
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&scroller, Some("contact"));
    stack.set_vexpand(true);
    root.append(&stack);

    ContactPane {
        root,
        stack,
        avatar_slot,
        name,
        address,
        reply,
        compose,
        copy,
        find,
        about,
        folders,
        folders_heading,
        recent_heading,
        recent,
    }
}

fn tile(icon: &str, tooltip: &str) -> gtk::Button {
    let button = theme::icon_button(icon, tooltip);
    button.add_css_class("tile");
    button
}

pub fn clear(ui: &Ui) {
    ui.contact.stack.set_visible_child_name("empty");
}

/// Fill the card for the message now open in the reading pane.
pub fn update(state: &Rc<RefCell<AppState>>, ui: &Ui, detail: &MessageDetail) {
    let name = display_sender(&detail.summary.from);
    let address = sender_address(&detail.summary.from);

    ui.contact.name.set_text(&name);
    ui.contact.address.set_text(&address);

    while let Some(child) = ui.contact.avatar_slot.first_child() {
        ui.contact.avatar_slot.remove(&child);
    }
    ui.contact.avatar_slot.append(&theme::avatar(&name, 56));

    // Everything below comes out of the cache: how much of their mail is on
    // this machine, where it lives, and what the last few pieces were.
    let (stats, history) = {
        let state = state.borrow();
        (
            state.db().sender_stats(&address).unwrap_or_default(),
            state
                .db()
                .messages_from(&address, RECENT + 1)
                .unwrap_or_default(),
        )
    };

    clear_box(&ui.contact.about);
    ui.contact.about.append(&fact(
        "mail-read-symbolic",
        &match stats.messages {
            1 => "1 message on this computer".to_string(),
            n => format!("{n} messages on this computer"),
        },
    ));
    if let Some(first) = stats.first {
        ui.contact.about.append(&fact(
            "document-open-recent-symbolic",
            &format!(
                "First heard from {}",
                first.with_timezone(&chrono::Local).format("%b %-d, %Y")
            ),
        ));
    }
    if let Some(last) = stats.last {
        ui.contact.about.append(&fact(
            "alarm-symbolic",
            &format!(
                "Last wrote {}",
                last.with_timezone(&chrono::Local).format("%b %-d, %Y")
            ),
        ));
    }

    // The folders their mail landed in, each one a way into that folder.
    let mut seen_folders: Vec<String> = Vec::new();
    for message in &history {
        if !seen_folders.contains(&message.folder_name) {
            seen_folders.push(message.folder_name.clone());
        }
    }
    clear_box(&ui.contact.folders);
    ui.contact
        .folders_heading
        .set_visible(!seen_folders.is_empty());
    ui.contact.folders.set_visible(!seen_folders.is_empty());
    for folder_name in &seen_folders {
        let folder_id = state.borrow().folder_id_by_name(folder_name);
        let row = link_row("folder-symbolic", leaf_name(folder_name), None);
        if let Some(id) = folder_id {
            let state = state.clone();
            let ui = ui.clone();
            let name = folder_name.clone();
            row.connect_clicked(move |_| {
                crate::ui::mailbox::select_view(&state, &ui, View::Folder(id, name.clone()));
            });
        } else {
            row.set_sensitive(false);
        }
        ui.contact.folders.append(&row);
    }

    // Their other mail, newest first, minus the one already open.
    let others: Vec<_> = history
        .iter()
        .filter(|m| m.id != detail.summary.id)
        .take(RECENT)
        .collect();
    ui.contact
        .recent_heading
        .set_text(&format!("Recent from {}", shorten(&name)));
    ui.contact.recent_heading.set_visible(!others.is_empty());
    clear_box(&ui.contact.recent);
    ui.contact.recent.set_visible(!others.is_empty());
    for message in others {
        let row = link_row(
            "mail-read-symbolic",
            crate::ui::mailbox::subject_or_placeholder(&message.subject),
            message.date.map(format_date),
        );
        let state = state.clone();
        let ui_inner = ui.clone();
        let id = message.id;
        row.connect_clicked(move |_| crate::ui::mailbox::open_message(&state, &ui_inner, id));
        ui.contact.recent.append(&row);
    }

    ui.contact.stack.set_visible_child_name("contact");
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// One line of the About block: a symbol and a sentence.
fn fact(icon: &str, text: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(14);
    image.add_css_class("faint");
    image.set_valign(gtk::Align::Start);
    row.append(&image);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("small");
    label.add_css_class("muted");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_hexpand(true);
    row.append(&label);
    row
}

/// A clickable line in the Folders or Recent lists: symbol, title, and an
/// optional date under it.
fn link_row(icon: &str, title: &str, subtitle: Option<String>) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(14);
    image.set_valign(gtk::Align::Start);
    image.set_margin_top(2);
    content.append(&image);

    let words = gtk::Box::new(gtk::Orientation::Vertical, 1);
    words.set_hexpand(true);
    let label = gtk::Label::new(Some(title));
    label.add_css_class("small");
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    words.append(&label);
    if let Some(subtitle) = subtitle {
        let under = gtk::Label::new(Some(&subtitle));
        under.add_css_class("tiny");
        under.add_css_class("faint");
        under.set_xalign(0.0);
        words.append(&under);
    }
    content.append(&words);

    let row = gtk::Button::builder().child(&content).build();
    row.add_css_class("flat");
    row.add_css_class("nav-row");
    row.set_tooltip_text(Some(title));
    row
}

/// The heading has a whole name in it and a narrow column to sit in.
fn shorten(name: &str) -> String {
    let first = name.split_whitespace().next().unwrap_or(name);
    if first.chars().count() > 14 {
        first.chars().take(13).chain(['…']).collect()
    } else {
        first.to_string()
    }
}
