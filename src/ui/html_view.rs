//! The reading pane's view for HTML mail: a WebKit web view locked down to
//! display a document and nothing more, plus the bar that offers to load
//! remote images.
//!
//! What it will not do: run scripts, load anything from the network the
//! message's policy does not allow (see `crate::html`), keep cookies or
//! storage between messages, or navigate. A clicked link goes to the default
//! browser instead of replacing the message.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use webkit6::prelude::*;

/// A floor under every message's own policy, fixed when the view is built so
/// no document can loosen it: no scripts, frames, plugins, forms or requests
/// from script, whatever the per-message policy says about images.
const HARD_POLICY: &str = "script-src 'none'; object-src 'none'; frame-src 'none'; \
     connect-src 'none'; form-action 'none'; base-uri 'none'";

pub struct HtmlView {
    /// The bar and the web view together; what goes in the pane.
    pub root: gtk::Box,
    web: webkit6::WebView,
    images_bar: gtk::Revealer,
    /// The message on screen, kept for reloading it with images allowed.
    current: Rc<RefCell<String>>,
}

impl Default for HtmlView {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlView {
    pub fn new() -> Self {
        let settings = webkit6::Settings::new();
        settings.set_enable_javascript(false);
        settings.set_enable_javascript_markup(false);
        settings.set_enable_html5_local_storage(false);
        settings.set_enable_html5_database(false);
        settings.set_enable_page_cache(false);
        settings.set_enable_webgl(false);
        settings.set_enable_developer_extras(false);
        // Images the policy allows (inline ones, or remote once asked for).
        settings.set_auto_load_images(true);

        let web = webkit6::WebView::builder()
            .settings(&settings)
            .network_session(&webkit6::NetworkSession::new_ephemeral())
            .default_content_security_policy(HARD_POLICY)
            .vexpand(true)
            .hexpand(true)
            .build();
        // Matches the page's own background, so nothing flashes dark while a
        // message loads.
        web.set_background_color(&gtk::gdk::RGBA::WHITE);
        web.add_css_class("reading-html");

        web.connect_decide_policy(|_, decision, kind| {
            if !matches!(
                kind,
                webkit6::PolicyDecisionType::NavigationAction
                    | webkit6::PolicyDecisionType::NewWindowAction
            ) {
                return false;
            }
            let Some(action) = decision
                .downcast_ref::<webkit6::NavigationPolicyDecision>()
                .and_then(|d| d.navigation_action())
            else {
                return false;
            };
            let uri = action
                .request()
                .and_then(|r| r.uri())
                .map(|u| u.to_string())
                .unwrap_or_default();
            // `load_html` itself, and in-page anchors, stay here.
            if uri.is_empty() || uri.starts_with("about:") {
                return false;
            }
            decision.ignore();
            if action.is_user_gesture()
                && let Err(e) = gtk::gio::AppInfo::launch_default_for_uri(
                    &uri,
                    None::<&gtk::gio::AppLaunchContext>,
                )
            {
                tracing::warn!("could not open {uri}: {e}");
            }
            true
        });

        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        bar.add_css_class("images-bar");
        bar.set_margin_bottom(8);
        let note = gtk::Label::new(Some(
            "Remote images are blocked so the sender can't tell you opened this.",
        ));
        note.add_css_class("small");
        note.add_css_class("muted");
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_hexpand(true);
        bar.append(&note);
        let load = gtk::Button::with_label("Load images");
        load.add_css_class("small");
        bar.append(&load);
        let images_bar = gtk::Revealer::builder().child(&bar).build();

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("html-card");
        card.set_overflow(gtk::Overflow::Hidden);
        card.append(&web);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&images_bar);
        root.append(&card);

        let current = Rc::new(RefCell::new(String::new()));
        {
            let web = web.clone();
            let images_bar = images_bar.clone();
            let current = current.clone();
            load.connect_clicked(move |_| {
                web.load_html(&crate::html::prepare(&current.borrow(), true), None);
                images_bar.set_reveal_child(false);
            });
        }

        Self {
            root,
            web,
            images_bar,
            current,
        }
    }

    /// Show a message, remote images blocked.
    pub fn show(&self, body_html: &str) {
        *self.current.borrow_mut() = body_html.to_string();
        self.images_bar
            .set_reveal_child(crate::html::has_remote_content(body_html));
        self.web
            .load_html(&crate::html::prepare(body_html, false), None);
    }
}
