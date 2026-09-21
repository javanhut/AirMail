//! The compose window. Plain-text only in v1.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::ui::{Ui, theme};

pub struct SendRequest {
    pub account_email: String,
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// What the composer opens with. A blank one is a new message; `reply_to`
/// fills it in from an open message.
#[derive(Debug, Clone, Default)]
pub struct Prefill {
    pub to: String,
    pub subject: String,
    pub body: String,
}

impl Prefill {
    /// A reply: their address, `Re:` once and only once, and the message
    /// quoted underneath with a blank line to type into above it.
    pub fn reply_to(detail: &crate::models::MessageDetail) -> Self {
        let subject = detail.summary.subject.trim();
        let subject = if subject.to_lowercase().starts_with("re:") {
            subject.to_string()
        } else {
            format!("Re: {subject}")
        };
        let quoted: String = detail
            .body_text
            .lines()
            .take(QUOTE_LINES)
            .map(|line| format!("> {line}\n"))
            .collect();
        Self {
            to: crate::ui::mailbox::sender_address(&detail.summary.from),
            subject,
            body: format!(
                "\n\nOn {}, {} wrote:\n{quoted}",
                detail
                    .summary
                    .date
                    .map(|d| d
                        .with_timezone(&chrono::Local)
                        .format("%b %-d, %Y at %-I:%M %p")
                        .to_string())
                    .unwrap_or_else(|| "an earlier date".to_string()),
                crate::ui::mailbox::display_sender(&detail.summary.from),
            ),
        }
    }
}

/// How much of the original a reply quotes. Long enough to give context,
/// short enough that the reply window is not someone else's newsletter.
const QUOTE_LINES: usize = 40;

/// Open the composer over `ui.window`. `on_send` is called once, with the
/// finished message, if the user sends; the dialog closes itself either way.
pub fn present(
    accounts: &[String],
    prefill: Prefill,
    ui: &Ui,
    on_send: impl Fn(SendRequest) + 'static,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title(if prefill.subject.is_empty() {
        "New message"
    } else {
        "Reply"
    });
    dialog.set_content_width(620);
    dialog.set_content_height(520);

    let header = adw::HeaderBar::new();
    let cancel = gtk::Button::with_label("Cancel");
    header.pack_start(&cancel);
    let send = theme::primary_button("Send");
    send.set_sensitive(false);
    header.pack_end(&send);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    theme::set_margins(&content, 14);

    let fields = adw::PreferencesGroup::new();
    let from = adw::ComboRow::new();
    from.set_title("From");
    let model = gtk::StringList::new(&accounts.iter().map(String::as_str).collect::<Vec<_>>());
    from.set_model(Some(&model));
    from.set_selected(0);
    fields.add(&from);

    let to = adw::EntryRow::builder().title("To").build();
    to.set_text(&prefill.to);
    fields.add(&to);
    let subject = adw::EntryRow::builder().title("Subject").build();
    subject.set_text(&prefill.subject);
    fields.add(&subject);
    content.append(&fields);

    let body = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(10)
        .right_margin(10)
        .top_margin(10)
        .bottom_margin(10)
        .build();
    body.add_css_class("compose");
    body.buffer().set_text(&prefill.body);
    // A reply starts with two blank lines above the quote; put the cursor in
    // them rather than at the end of somebody else's words.
    body.buffer().place_cursor(&body.buffer().start_iter());
    let body_frame = theme::card();
    body_frame.append(&body);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body_frame)
        .build();
    content.append(&scroller);

    let footer = gtk::Label::new(Some("Plain text"));
    footer.add_css_class("small");
    footer.add_css_class("faint");
    footer.set_xalign(0.0);
    content.append(&footer);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));

    // Send stays out of reach until there is somewhere to send to and
    // something to say it is about — the same rule the egui composer used.
    let refresh_send = {
        let to = to.clone();
        let subject = subject.clone();
        let send = send.clone();
        let accounts = accounts.to_vec();
        let from = from.clone();
        Rc::new(move || {
            let has_account = accounts.get(from.selected() as usize).is_some();
            send.set_sensitive(
                has_account && !to.text().trim().is_empty() && !subject.text().trim().is_empty(),
            );
        })
    };
    for entry in [&to, &subject] {
        let refresh_send = refresh_send.clone();
        entry.connect_changed(move |_| refresh_send());
    }
    {
        let refresh_send = refresh_send.clone();
        from.connect_selected_notify(move |_| refresh_send());
    }
    refresh_send();

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }

    let accounts = accounts.to_vec();
    let on_send = Rc::new(on_send);
    let dialog_for_send = dialog.clone();
    let sent = Rc::new(RefCell::new(false));
    send.connect_clicked(move |_| {
        if *sent.borrow() {
            return;
        }
        let Some(account_email) = accounts.get(from.selected() as usize).cloned() else {
            return;
        };
        let buffer = body.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        *sent.borrow_mut() = true;
        on_send(SendRequest {
            account_email,
            to: to.text().trim().to_string(),
            subject: subject.text().trim().to_string(),
            body: text,
        });
        dialog_for_send.close();
    });

    dialog.present(Some(&ui.window));
}
