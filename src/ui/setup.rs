//! Adding an account: pick who hosts the mail, type the address and password.
//! Server details are only ever shown for "Other", and even then they come
//! pre-filled.
//!
//! The decisions — which provider a typed address implies, whether the form is
//! complete, what `AccountConfig` it turns into — live in `SetupForm`, which
//! holds no widgets. `present` is the GTK shell around it. That split is what
//! keeps the dialog testable with no display attached.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use anyhow::{Context, Result};

use crate::models::{AccountConfig, OAuthProvider, SmtpSecurity};
use crate::providers::{self, Provider};
use crate::ui::{Ui, theme};

/// Which set of server settings the account will use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// One of the providers we ship settings for.
    Known(&'static Provider),
    /// Anything else: hosts and ports are guessed from the domain and shown
    /// for correction.
    Other,
}

/// Editable host/port fields, only reachable through "Other".
#[derive(Default, Debug, Clone)]
pub struct CustomServers {
    pub imap_host: String,
    pub imap_port: String,
    pub smtp_host: String,
    pub smtp_port: String,
    pub smtp_security: SmtpSecurity,
    /// Whether the fields still hold values derived from the typed domain.
    pub derived: bool,
}

/// The state behind the dialog, with no widgets in it.
pub struct SetupForm {
    choice: Option<Choice>,
    /// True once the provider was picked by hand, so typing an address stops
    /// moving the selection out from under the pointer.
    choice_is_manual: bool,
    email: String,
    password: String,
    custom: CustomServers,
    /// Providers this machine has a client ID for, so they sign in through
    /// the browser instead of taking a password. Empty by default, which keeps
    /// the form's behaviour independent of whatever `oauth.toml` holds.
    browser_sign_in: Vec<OAuthProvider>,
}

impl Default for SetupForm {
    fn default() -> Self {
        Self {
            choice: None,
            choice_is_manual: false,
            email: String::new(),
            password: String::new(),
            custom: CustomServers {
                derived: true,
                ..Default::default()
            },
            browser_sign_in: Vec::new(),
        }
    }
}

impl SetupForm {
    /// A form that offers browser sign-in wherever this machine can do it.
    pub fn for_this_machine() -> Self {
        Self::default().with_browser_sign_in(
            [OAuthProvider::Google, OAuthProvider::Microsoft]
                .into_iter()
                .filter(|p| crate::oauth::is_configured(*p))
                .collect(),
        )
    }

    pub fn with_browser_sign_in(mut self, providers: Vec<OAuthProvider>) -> Self {
        self.browser_sign_in = providers;
        self
    }

    /// The provider to sign in to through the browser, when the chosen host
    /// offers it here. `None` means the form wants a password.
    pub fn browser_provider(&self) -> Option<OAuthProvider> {
        match self.choice {
            Some(Choice::Known(provider)) => {
                provider.oauth.filter(|p| self.browser_sign_in.contains(p))
            }
            _ => None,
        }
    }

    pub fn choice(&self) -> Option<Choice> {
        self.choice
    }

    pub fn custom(&self) -> &CustomServers {
        &self.custom
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    /// True when the dialog has enough to try saving.
    pub fn is_ready(&self) -> bool {
        self.choice.is_some()
            && !self.email.trim().is_empty()
            && (self.browser_provider().is_some() || !self.password.is_empty())
    }

    /// The hint shown under the password field, as Pango markup: what the
    /// provider wants instead of the web password and, where browser sign-in
    /// exists but is not set up on this machine, how to set it up.
    pub fn hint(&self) -> Option<String> {
        if self.browser_provider().is_some() {
            return Some(
                "You'll sign in in your browser. AirMail never sees your password.".into(),
            );
        }
        let Some(Choice::Known(provider)) = self.choice else {
            return None;
        };
        let password = provider
            .hint
            .map(|text| gtk::glib::markup_escape_text(text).to_string());
        let setup = provider.oauth.map(|oauth| oauth.setup_steps().to_string());
        match (password, setup) {
            (Some(password), Some(setup)) => Some(format!("{password}\n\n{setup}")),
            (one, other) => one.or(other),
        }
    }

    pub fn set_email(&mut self, email: &str) {
        self.email = email.to_string();
        self.auto_select_provider();
    }

    pub fn set_password(&mut self, password: &str) {
        self.password = password.to_string();
    }

    /// Pick a provider by hand. From here on the typed address stops moving
    /// the selection.
    pub fn choose(&mut self, choice: Choice) {
        self.choice = Some(choice);
        self.choice_is_manual = true;
        if choice == Choice::Other {
            self.refresh_derived_servers();
        }
    }

    /// Record a typed server field. Typing into any of them means the values
    /// are no longer guesses, so the address stops overwriting them.
    pub fn set_custom(&mut self, edit: impl FnOnce(&mut CustomServers)) {
        edit(&mut self.custom);
        self.custom.derived = false;
    }

    /// Follow the typed domain until the provider is picked by hand. Keeps the
    /// common case down to two fields.
    fn auto_select_provider(&mut self) {
        if self.choice_is_manual {
            if self.custom.derived {
                self.refresh_derived_servers();
            }
            return;
        }
        match providers::for_email(&self.email) {
            Some(provider) => self.choice = Some(Choice::Known(provider)),
            None => {
                if providers::domain_of(&self.email).is_some() {
                    self.choice = Some(Choice::Other);
                    self.refresh_derived_servers();
                } else {
                    self.choice = None;
                }
            }
        }
    }

    /// Re-fill the custom host fields from the address, as long as they still
    /// hold guessed values rather than something typed.
    fn refresh_derived_servers(&mut self) {
        if !self.custom.derived {
            return;
        }
        let guess = providers::guess_config(&self.email);
        self.custom.imap_host = guess.imap_host;
        self.custom.imap_port = guess.imap_port.to_string();
        self.custom.smtp_host = guess.smtp_host;
        self.custom.smtp_port = guess.smtp_port.to_string();
        self.custom.smtp_security = guess.smtp_security;
    }

    pub fn validate(&self) -> Result<()> {
        if providers::domain_of(&self.email).is_none() {
            anyhow::bail!("enter a full email address, like you@example.com");
        }
        if self.password.is_empty() && self.browser_provider().is_none() {
            anyhow::bail!("a password is required");
        }
        match self.choice {
            None => anyhow::bail!("choose where this mail is hosted"),
            Some(Choice::Known(_)) => Ok(()),
            Some(Choice::Other) => {
                if self.custom.imap_host.trim().is_empty()
                    || self.custom.smtp_host.trim().is_empty()
                {
                    anyhow::bail!("incoming and outgoing server names are required");
                }
                self.custom
                    .imap_port
                    .trim()
                    .parse::<u16>()
                    .context("the IMAP port must be a number between 1 and 65535")?;
                self.custom
                    .smtp_port
                    .trim()
                    .parse::<u16>()
                    .context("the SMTP port must be a number between 1 and 65535")?;
                Ok(())
            }
        }
    }

    /// The finished account plus the password to hand to the keyring. For a
    /// browser sign-in the password is empty and `oauth` is set; the refresh
    /// token takes its place once the browser comes back.
    pub fn save(self) -> Result<(AccountConfig, String)> {
        self.validate()?;
        let email = self.email.trim().to_string();
        let browser = self.browser_provider();
        let config = match self.choice {
            Some(Choice::Known(provider)) => AccountConfig {
                oauth: browser,
                ..provider.account_config(&email)
            },
            Some(Choice::Other) => AccountConfig {
                imap_host: self.custom.imap_host.trim().to_string(),
                imap_port: self.custom.imap_port.trim().parse()?,
                smtp_host: self.custom.smtp_host.trim().to_string(),
                smtp_port: self.custom.smtp_port.trim().parse()?,
                smtp_security: self.custom.smtp_security,
                ..providers::guess_config(&email)
            },
            None => anyhow::bail!("choose where this mail is hosted"),
        };
        let password = if browser.is_some() {
            String::new()
        } else {
            self.password
        };
        Ok((config, password))
    }
}

/// A server entry paired with where its text belongs in `CustomServers`.
type ServerRow = (adw::EntryRow, fn(&mut CustomServers, String));

/// Every choice the tiles offer, providers first and "Other" last.
fn choices() -> Vec<Choice> {
    let mut choices: Vec<Choice> = providers::PROVIDERS.iter().map(Choice::Known).collect();
    choices.push(Choice::Other);
    choices
}

fn label_for(choice: Choice) -> &'static str {
    match choice {
        Choice::Known(provider) => provider.name,
        Choice::Other => "Other",
    }
}

/// Open the setup dialog over `ui.window`. `on_save` receives the finished
/// account and its password.
pub fn present(ui: &Ui, on_save: impl Fn(AccountConfig, String) + 'static) {
    let form = Rc::new(RefCell::new(SetupForm::for_this_machine()));
    // Set while the code writes into the entries, so the handlers below can
    // tell a programmatic update from something the user typed.
    let updating = Rc::new(Cell::new(false));

    let dialog = adw::Dialog::new();
    dialog.set_title("Add an account");
    dialog.set_content_width(480);

    let header = adw::HeaderBar::new();
    let cancel = gtk::Button::with_label("Cancel");
    header.pack_start(&cancel);
    let add = theme::primary_button("Add account");
    add.set_sensitive(false);
    header.pack_end(&add);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    theme::set_margins(&content, 16);

    let prompt = gtk::Label::new(Some("Where is this mail hosted?"));
    prompt.add_css_class("small");
    prompt.add_css_class("muted");
    prompt.set_xalign(0.0);
    content.append(&prompt);

    // The provider tiles. Three per row, with "Other" last.
    let tiles_box = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(3)
        .max_children_per_line(3)
        .row_spacing(6)
        .column_spacing(6)
        .homogeneous(true)
        .build();
    let tiles: Vec<gtk::ToggleButton> = choices()
        .into_iter()
        .map(|choice| {
            let tile = gtk::ToggleButton::new();
            tile.add_css_class("tile");

            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            dot.set_valign(gtk::Align::Center);
            dot.add_css_class("avatar");
            dot.add_css_class("avatar-s10");
            dot.add_css_class(&format!(
                "avatar-{}",
                theme::avatar_color_index(label_for(choice))
            ));
            row.append(&dot);
            let name = gtk::Label::new(Some(label_for(choice)));
            name.set_xalign(0.0);
            row.append(&name);
            tile.set_child(Some(&row));
            tiles_box.insert(&tile, -1);
            tile
        })
        .collect();
    content.append(&tiles_box);
    content.append(&theme::rule());

    let credentials = adw::PreferencesGroup::new();
    let email = adw::EntryRow::builder().title("Email address").build();
    credentials.add(&email);
    let password = adw::PasswordEntryRow::builder().title("Password").build();
    credentials.add(&password);
    content.append(&credentials);

    let hint = gtk::Label::new(None);
    hint.add_css_class("small");
    hint.add_css_class("faint");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.set_visible(false);
    content.append(&hint);

    // Host and port fields, revealed only for "Other".
    let servers = adw::PreferencesGroup::new();
    servers.set_title("Server settings");
    let imap_host = adw::EntryRow::builder().title("Incoming (IMAP)").build();
    let imap_port = adw::EntryRow::builder().title("IMAP port").build();
    let smtp_host = adw::EntryRow::builder().title("Outgoing (SMTP)").build();
    let smtp_port = adw::EntryRow::builder().title("SMTP port").build();
    let security = adw::ComboRow::new();
    security.set_title("Encryption");
    security.set_subtitle("TLS is encrypted from the start; STARTTLS upgrades after connecting");
    security.set_model(Some(&gtk::StringList::new(&[
        SmtpSecurity::Tls.label(),
        SmtpSecurity::StartTls.label(),
    ])));
    for row in [&imap_host, &imap_port, &smtp_host, &smtp_port] {
        servers.add(row);
    }
    servers.add(&security);
    let revealer = gtk::Revealer::builder().child(&servers).build();
    content.append(&revealer);

    let error = gtk::Label::new(None);
    error.add_css_class("small");
    error.add_css_class("danger");
    error.set_xalign(0.0);
    error.set_wrap(true);
    error.set_visible(false);
    content.append(&error);

    let footer = gtk::Label::new(Some(
        "Your password or sign-in is stored in HuginnKeyring, the system keyring, never on disk.",
    ));
    footer.add_css_class("small");
    footer.add_css_class("faint");
    footer.set_xalign(0.0);
    footer.set_wrap(true);
    content.append(&footer);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&content)
            .build(),
    ));
    dialog.set_child(Some(&toolbar));

    // Push form state into the widgets. Runs after every change, and is the
    // only thing that writes into the entries.
    let refresh: Rc<dyn Fn()> = {
        let form = form.clone();
        let updating = updating.clone();
        let tiles = tiles.clone();
        let revealer = revealer.clone();
        let hint = hint.clone();
        let add = add.clone();
        let imap_host = imap_host.clone();
        let imap_port = imap_port.clone();
        let smtp_host = smtp_host.clone();
        let smtp_port = smtp_port.clone();
        let security = security.clone();
        let password = password.clone();
        Rc::new(move || {
            let form = form.borrow();
            updating.set(true);

            for (tile, choice) in tiles.iter().zip(choices()) {
                tile.set_active(form.choice() == Some(choice));
            }

            let is_other = form.choice() == Some(Choice::Other);
            revealer.set_reveal_child(is_other);
            if is_other {
                let custom = form.custom();
                for (row, value) in [
                    (&imap_host, &custom.imap_host),
                    (&imap_port, &custom.imap_port),
                    (&smtp_host, &custom.smtp_host),
                    (&smtp_port, &custom.smtp_port),
                ] {
                    if row.text() != *value {
                        row.set_text(value);
                    }
                }
                security.set_selected(match custom.smtp_security {
                    SmtpSecurity::Tls => 0,
                    SmtpSecurity::StartTls => 1,
                });
            }

            match form.hint() {
                Some(text) => {
                    hint.set_markup(&text);
                    hint.set_visible(true);
                }
                None => hint.set_visible(false),
            }

            let browser = form.browser_provider();
            password.set_visible(browser.is_none());
            add.set_label(&match browser {
                Some(provider) => format!("Sign in with {}", provider.label()),
                None => "Add account".to_string(),
            });

            add.set_sensitive(form.is_ready());
            updating.set(false);
        })
    };

    for (index, tile) in tiles.iter().enumerate() {
        let form = form.clone();
        let refresh = refresh.clone();
        let updating = updating.clone();
        let error = error.clone();
        tile.connect_toggled(move |tile| {
            if updating.get() || !tile.is_active() {
                return;
            }
            if let Some(choice) = choices().get(index).copied() {
                form.borrow_mut().choose(choice);
            }
            error.set_visible(false);
            refresh();
        });
    }

    {
        let form = form.clone();
        let refresh = refresh.clone();
        let updating = updating.clone();
        let error = error.clone();
        email.connect_changed(move |entry| {
            if updating.get() {
                return;
            }
            form.borrow_mut().set_email(&entry.text());
            error.set_visible(false);
            refresh();
        });
    }
    {
        let form = form.clone();
        let refresh = refresh.clone();
        let updating = updating.clone();
        let error = error.clone();
        password.connect_changed(move |entry| {
            if updating.get() {
                return;
            }
            form.borrow_mut().set_password(&entry.text());
            error.set_visible(false);
            refresh();
        });
    }

    // Typed server fields. Each marks the custom settings as no longer derived
    // from the address.
    let server_rows: [ServerRow; 4] = [
        (imap_host.clone(), |c, v| c.imap_host = v),
        (imap_port.clone(), |c, v| c.imap_port = v),
        (smtp_host.clone(), |c, v| c.smtp_host = v),
        (smtp_port.clone(), |c, v| c.smtp_port = v),
    ];
    for (row, assign) in server_rows {
        let form = form.clone();
        let refresh = refresh.clone();
        let updating = updating.clone();
        let error = error.clone();
        row.connect_changed(move |entry| {
            if updating.get() {
                return;
            }
            let value = entry.text().to_string();
            form.borrow_mut().set_custom(|custom| assign(custom, value));
            error.set_visible(false);
            refresh();
        });
    }
    {
        let form = form.clone();
        let refresh = refresh.clone();
        let updating = updating.clone();
        security.connect_selected_notify(move |combo| {
            if updating.get() {
                return;
            }
            let selected = match combo.selected() {
                1 => SmtpSecurity::StartTls,
                _ => SmtpSecurity::Tls,
            };
            form.borrow_mut()
                .set_custom(|custom| custom.smtp_security = selected);
            refresh();
        });
    }

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }

    let on_save = Rc::new(on_save);
    {
        let form = form.clone();
        let dialog = dialog.clone();
        let error = error.clone();
        add.connect_clicked(move |_| {
            if let Err(e) = form.borrow().validate() {
                error.set_text(&format!("{e:#}"));
                error.set_visible(true);
                return;
            }
            // `save` consumes the form, so it is swapped out for a fresh one.
            let finished = std::mem::take(&mut *form.borrow_mut());
            match finished.save() {
                Ok((config, password)) => {
                    dialog.close();
                    on_save(config, password);
                }
                Err(e) => {
                    error.set_text(&format!("Could not add the account: {e:#}"));
                    error.set_visible(true);
                }
            }
        });
    }

    refresh();
    dialog.present(Some(&ui.window));
}
