//! The Raven palette and the handful of widgets built on top of it.
//!
//! Everything visual lives here so the panels stay about mail: the rest of the
//! UI asks for a `.surface` style class or `theme::avatar()` rather than
//! mixing its own colours. Under GTK the palette is a stylesheet rather than a
//! set of paint calls, so the constants below exist to be interpolated into
//! `css()` — which is a pure function, and therefore testable without a
//! display.

use adw::prelude::*;

// Backgrounds, darkest first. The window sits on near-black navy; panels and
// cards step up toward the reader.
pub const BG_DEEP: &str = "#080B14";
pub const PANEL: &str = "#0E1320";
pub const SURFACE: &str = "#141A2A";
pub const SURFACE_HOVER: &str = "#1B2336";
pub const SURFACE_ACTIVE: &str = "#222C44";

// The blue that carries every primary action, plus the wash behind a selected
// row (the same hue at low weight, so selection reads as "lit", not "boxed").
pub const ACCENT: &str = "#3B82F6";
pub const ACCENT_HOVER: &str = "#609CFA";
pub const ACCENT_PRESSED: &str = "#2563EB";
pub const SELECTED_BG: &str = "#1C2C4E";
pub const SELECTED_EDGE: &str = "#395B9E";

pub const BORDER: &str = "#1E273A";
pub const BORDER_SOFT: &str = "#171E2E";

pub const TEXT: &str = "#E8ECF4";
pub const TEXT_MUTED: &str = "#949EB4";
pub const TEXT_FAINT: &str = "#646E85";

pub const DANGER: &str = "#F87171";
pub const WARN: &str = "#FBBF24";
pub const SUCCESS: &str = "#4ADE80";

/// Avatar colours, picked per correspondent so a sender keeps the same dot.
pub const AVATARS: &[&str] = &[
    "#607DF6", "#A78BFA", "#34D399", "#F472B6", "#FBBF24", "#38BDF8", "#FB7185", "#4ADE80",
];

/// Diameters avatars are drawn at. Each gets a rule in `css()`, because a
/// circle's radius has to track its size and GTK has no way to say "half of
/// whatever this is" in CSS.
pub const AVATAR_SIZES: &[i32] = &[10, 22, 32, 34];

pub const RADIUS: u8 = 10;
pub const RADIUS_SMALL: u8 = 7;

/// Install the palette on the default display and force the dark scheme.
/// Called once at startup. AirMail is dark only, so libadwaita is told not to
/// follow the desktop's preference.
pub fn apply() {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);

    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css());
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// The whole stylesheet, as a string. Built here rather than shipped as a
/// static `.css` file so the palette constants stay the single source of
/// truth, and so a test can check it without opening a display.
pub fn css() -> String {
    let mut css = format!(
        "
window.airmail, .bg-deep {{ background-color: {BG_DEEP}; color: {TEXT}; }}
.panel {{ background-color: {PANEL}; }}
.surface {{
    background-color: {SURFACE};
    border: 1px solid {BORDER};
    border-radius: {RADIUS}px;
}}
headerbar {{ background-color: {BG_DEEP}; box-shadow: inset 0 -1px {BORDER_SOFT}; }}

/* Type scale. The egui UI sized text per label; here it is class-driven. */
.title {{ font-size: 17px; font-weight: 700; color: {TEXT}; }}
.subject {{ font-size: 19px; font-weight: 700; color: {TEXT}; }}
.heading-sm {{ font-size: 15px; font-weight: 700; color: {TEXT}; }}
.muted {{ color: {TEXT_MUTED}; }}
.faint {{ color: {TEXT_FAINT}; }}
.small {{ font-size: 11.5px; }}
.danger {{ color: {DANGER}; }}
.section-label {{ font-size: 11px; font-weight: 700; color: {TEXT_FAINT}; }}

/* Sidebar and message rows: the rounded highlight the painted rows had. */
.nav-row, .message-row {{
    border-radius: {RADIUS_SMALL}px;
    border: 1px solid transparent;
    background-color: transparent;
}}
.nav-row:hover, .message-row:hover {{ background-color: {SURFACE_HOVER}; }}
.nav-row:selected, .nav-row.selected,
.message-row:selected, .message-row.selected {{
    background-color: {SELECTED_BG};
    border-color: {SELECTED_EDGE};
}}
.message-row {{ border-radius: {RADIUS}px; }}
.nav-row label {{ color: {TEXT_MUTED}; }}
.nav-row:selected label, .nav-row.selected label {{ color: {TEXT}; }}
.unread label.sender, .unread label.subject {{ color: {TEXT}; }}
label.sender, label.subject {{ color: {TEXT_MUTED}; }}
.unread-dot {{ background-color: {ACCENT}; border-radius: 4px; min-width: 7px; min-height: 7px; }}

/* Counts, as on the sidebar's unread badges. */
.count-badge {{
    background-color: {SURFACE_HOVER};
    color: {TEXT_MUTED};
    border-radius: 9px;
    padding: 1px 7px;
    font-size: 11px;
}}
.count-badge.highlight {{ background-color: {ACCENT}; color: #FFFFFF; }}

/* The one primary action in a view. */
button.primary {{
    background-image: none;
    background-color: {ACCENT};
    color: #FFFFFF;
    font-weight: 700;
    border: none;
    border-radius: {RADIUS_SMALL}px;
}}
button.primary:hover {{ background-color: {ACCENT_HOVER}; }}
button.primary:active {{ background-color: {ACCENT_PRESSED}; }}
button.primary:disabled {{ background-color: {SURFACE_ACTIVE}; color: {TEXT_FAINT}; }}
button.destructive {{ background-image: none; background-color: {DANGER}; color: #FFFFFF; border: none; }}

/* Provider tiles in the setup dialog. */
.tile {{
    background-color: {SURFACE};
    border: 1px solid {BORDER};
    border-radius: {RADIUS}px;
    padding: 10px 12px;
    color: {TEXT_MUTED};
}}
.tile:hover {{ background-color: {SURFACE_HOVER}; border-color: {SELECTED_EDGE}; color: {TEXT}; }}
.tile:checked {{ background-color: {SELECTED_BG}; border-color: {ACCENT}; color: {TEXT}; }}

textview.reading, textview.reading text {{
    background-color: {SURFACE};
    color: {TEXT};
    font-size: 13px;
}}
textview.compose, textview.compose text {{ background-color: {SURFACE}; color: {TEXT}; }}
.rule {{ background-color: {BORDER_SOFT}; min-height: 1px; }}
.statusbar {{ background-color: {BG_DEEP}; border-top: 1px solid {BORDER_SOFT}; }}
"
    );

    // One class per avatar colour: a tinted disc with the sender's initial,
    // matching the fill/stroke/text weights the painted version used.
    for (i, colour) in AVATARS.iter().enumerate() {
        css.push_str(&format!(
            ".avatar-{i} {{ background-color: alpha({colour}, 0.30); \
             border: 1px solid alpha({colour}, 0.75); color: {colour}; }}\n"
        ));
    }
    // One class per diameter, for the radius and the initial's size.
    for size in AVATAR_SIZES {
        css.push_str(&format!(
            ".avatar-s{size} {{ min-width: {size}px; min-height: {size}px; \
             border-radius: {radius}px; font-size: {font}px; font-weight: 700; }}\n",
            radius = size / 2,
            font = (f64::from(*size) * 0.44).round() as i32,
        ));
    }
    css
}

/// Stable per-sender colour index, so the same correspondent keeps one dot
/// colour across restarts (no hashing of pointers or row ids).
pub fn avatar_color_index(seed: &str) -> usize {
    let sum: u32 = seed
        .trim()
        .to_ascii_lowercase()
        .bytes()
        .fold(0u32, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(u32::from(b))
        });
    (sum as usize) % AVATARS.len()
}

/// The hex colour a sender's avatar is drawn in.
pub fn avatar_color(seed: &str) -> &'static str {
    AVATARS[avatar_color_index(seed)]
}

/// The letter shown in an avatar: first letter of the display name, or of the
/// address when the name is missing.
pub fn initial(name: &str) -> String {
    name.trim()
        .trim_start_matches(['"', '<', '\''])
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// A circular avatar with the sender's initial, at one of `AVATAR_SIZES`.
pub fn avatar(label: &str, diameter: i32) -> gtk::Label {
    debug_assert!(
        AVATAR_SIZES.contains(&diameter),
        "avatar size {diameter} has no rule in the stylesheet"
    );
    let avatar = gtk::Label::new(Some(&initial(label)));
    avatar.set_valign(gtk::Align::Center);
    avatar.set_halign(gtk::Align::Center);
    avatar.add_css_class("avatar");
    avatar.add_css_class(&format!("avatar-{}", avatar_color_index(label)));
    avatar.add_css_class(&format!("avatar-s{diameter}"));
    avatar
}

/// A filled blue button for the one primary action in a view.
pub fn primary_button(text: &str) -> gtk::Button {
    let button = gtk::Button::with_label(text);
    button.add_css_class("primary");
    button
}

/// A pill showing a count, as on the sidebar's unread badges.
pub fn count_badge(count: i64, highlight: bool) -> gtk::Label {
    let badge = gtk::Label::new(Some(&count.to_string()));
    badge.add_css_class("count-badge");
    if highlight {
        badge.add_css_class("highlight");
    }
    badge.set_valign(gtk::Align::Center);
    badge
}

/// The thin rule used between sections, softer than GTK's default separator.
pub fn rule() -> gtk::Separator {
    let rule = gtk::Separator::new(gtk::Orientation::Horizontal);
    rule.add_css_class("rule");
    rule
}

/// A raised, rounded container — the reading pane body, the setup card.
pub fn card() -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("surface");
    set_margins(&card, 14);
    card
}

/// Set equal margins on a widget, so the builders below read as one call
/// rather than four.
pub fn set_margins(widget: &impl IsA<gtk::Widget>, margin: i32) {
    let widget = widget.as_ref();
    widget.set_margin_top(margin);
    widget.set_margin_bottom(margin);
    widget.set_margin_start(margin);
    widget.set_margin_end(margin);
}
