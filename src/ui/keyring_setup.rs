//! The dialog for when there is nowhere to put a password yet.
//!
//! Raven keeps passwords in HuginnKeyring, and on a machine where the login
//! handoff has never run there is no keyring at all — nothing to store into
//! and nothing to unlock. Somebody has to choose the password that seals the
//! first one, and until a Raven prompter exists this is the only window in
//! front of the user at that moment.
//!
//! ## Why it asks for the login password
//!
//! Because the alternative traps people. The keyring this makes is called
//! `login`, the same one the handoff makes, so the day `ravend` or PAM starts
//! handing the login password over, that password is tried against this file.
//! Type the login password here and the keyring opens by itself from then on;
//! type something else and every session afterwards asks for a password the
//! user has forgotten they ever set. The copy says so plainly, and it is the
//! most important thing in the dialog.
//!
//! As in `setup`, the decisions live in [`NewKeyring`], which holds no
//! widgets, so they can be tested with no display attached.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::ui::{Ui, theme};

/// The two typed passwords, and whether they are worth sending.
#[derive(Default, Debug, Clone)]
pub struct NewKeyring {
    pub password: String,
    pub again: String,
}

impl NewKeyring {
    /// Whether the Create button should be clickable.
    ///
    /// Only emptiness, not agreement: a button that greys itself out the
    /// moment the second field falls behind the first reads as the dialog
    /// breaking under the user's hands. Disagreement is worth a sentence, and
    /// [`validate`](Self::validate) writes it.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.password.is_empty() && !self.again.is_empty()
    }

    /// The password to seal the keyring with, or what to tell the user.
    ///
    /// # Errors
    ///
    /// If nothing was typed, or the two fields disagree.
    pub fn validate(&self) -> Result<&str, &'static str> {
        if self.password.is_empty() {
            return Err("Type a password for the keyring.");
        }
        if self.password != self.again {
            return Err("Those two passwords do not match.");
        }
        Ok(&self.password)
    }
}

/// Ask for a password, make the keyring, and call `on_created` once there is
/// one.
///
/// `on_dismissed` runs if the dialog goes away without making one, by Cancel
/// or by Escape. The caller was part-way through something that cannot finish
/// without a keyring, and silence would leave a typed-out account looking like
/// it evaporated.
pub fn present(ui: &Ui, on_created: impl Fn() + 'static, on_dismissed: impl Fn() + 'static) {
    let form = Rc::new(RefCell::new(NewKeyring::default()));
    // Read by the close handler, which cannot otherwise tell "the user gave
    // up" from "the dialog closed because it worked".
    let created = Rc::new(std::cell::Cell::new(false));

    let dialog = adw::Dialog::new();
    dialog.set_title("No keyring yet");
    dialog.set_content_width(460);

    let header = adw::HeaderBar::new();
    let cancel = gtk::Button::with_label("Cancel");
    header.pack_start(&cancel);
    let create = theme::primary_button("Create keyring");
    create.set_sensitive(false);
    header.pack_end(&create);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    theme::set_margins(&content, 16);

    let blurb = gtk::Label::new(Some(
        "Your passwords live in HuginnKeyring, and this computer has no keyring \
         yet. AirMail can make one now.",
    ));
    blurb.set_xalign(0.0);
    blurb.set_wrap(true);
    content.append(&blurb);

    let entries = adw::PreferencesGroup::new();
    let password = adw::PasswordEntryRow::builder()
        .title("Keyring password")
        .build();
    entries.add(&password);
    let again = adw::PasswordEntryRow::builder().title("Again").build();
    entries.add(&again);
    content.append(&entries);

    let error = gtk::Label::new(None);
    error.add_css_class("small");
    error.add_css_class("danger");
    error.set_xalign(0.0);
    error.set_wrap(true);
    error.set_visible(false);
    content.append(&error);

    let advice = gtk::Label::new(Some(
        "Use the password you log in with. The keyring then opens by itself when \
         you log in, and nothing asks you for it again. A different password here \
         means typing it once every session.",
    ));
    advice.add_css_class("small");
    advice.add_css_class("faint");
    advice.set_xalign(0.0);
    advice.set_wrap(true);
    content.append(&advice);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));

    let refresh: Rc<dyn Fn()> = {
        let form = form.clone();
        let create = create.clone();
        Rc::new(move || create.set_sensitive(form.borrow().is_complete()))
    };

    for (entry, which) in [(&password, true), (&again, false)] {
        let form = form.clone();
        let refresh = refresh.clone();
        let error = error.clone();
        entry.connect_changed(move |entry| {
            let typed = entry.text().to_string();
            let mut form = form.borrow_mut();
            if which {
                form.password = typed;
            } else {
                form.again = typed;
            }
            drop(form);
            // Typing is the user answering the complaint; leaving it on screen
            // while they do only makes them read it twice.
            error.set_visible(false);
            refresh();
        });
    }

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }

    {
        let created = created.clone();
        dialog.connect_closed(move |_| {
            if !created.get() {
                on_dismissed();
            }
        });
    }

    {
        let form = form.clone();
        let dialog = dialog.clone();
        let error = error.clone();
        let created = created.clone();
        let on_created = Rc::new(on_created);
        create.connect_clicked(move |_| {
            let password = match form.borrow().validate() {
                Ok(password) => password.to_owned(),
                Err(complaint) => {
                    error.set_text(complaint);
                    error.set_visible(true);
                    return;
                }
            };
            match crate::keyring::create(&password) {
                Ok(()) => {
                    created.set(true);
                    dialog.close();
                    on_created();
                }
                Err(e) => {
                    let e = anyhow::Error::from(e);
                    error.set_text(&format!("Could not make the keyring: {e:#}"));
                    error.set_visible(true);
                }
            }
        });
    }

    dialog.present(Some(&ui.window));
}
