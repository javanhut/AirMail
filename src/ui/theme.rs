//! The Raven palette and the handful of widgets built on top of it.
//!
//! Everything visual lives here so the panels stay about mail: the rest of the
//! UI asks for a `.surface` style class or `theme::avatar()` rather than
//! mixing its own colours. Under GTK the palette is a stylesheet rather than a
//! set of paint calls, so the constants below exist to be interpolated into
//! `css()` — which is a pure function, and therefore testable without a
//! display.

use adw::prelude::*;

// Backgrounds, darkest first. The window sits on near-black navy; the three
// mail columns step up from it, and cards step up again toward the reader.
pub const BG_DEEP: &str = "#070A12";
pub const PANEL: &str = "#0C111C";
pub const PANEL_RAISED: &str = "#0F1524";
pub const SURFACE: &str = "#141B2B";
pub const SURFACE_HOVER: &str = "#1A2234";
pub const SURFACE_ACTIVE: &str = "#222C44";

// The blue that carries every primary action, plus the wash behind a selected
// row (the same hue at low weight, so selection reads as "lit", not "boxed").
pub const ACCENT: &str = "#3B82F6";
pub const ACCENT_HOVER: &str = "#609CFA";
pub const ACCENT_PRESSED: &str = "#2563EB";
pub const SELECTED_BG: &str = "#1C2C4E";
pub const SELECTED_EDGE: &str = "#3B5FA8";

pub const BORDER: &str = "#1C2435";
pub const BORDER_SOFT: &str = "#151C2A";

pub const TEXT: &str = "#E8ECF4";
pub const TEXT_MUTED: &str = "#98A2B8";
pub const TEXT_FAINT: &str = "#69748C";

pub const DANGER: &str = "#F87171";
pub const WARN: &str = "#FBBF24";
pub const SUCCESS: &str = "#4ADE80";
pub const STAR: &str = "#FBBF24";

/// Avatar colours, picked per correspondent so a sender keeps the same dot.
pub const AVATARS: &[&str] = &[
    "#607DF6", "#A78BFA", "#34D399", "#F472B6", "#FBBF24", "#38BDF8", "#FB7185", "#4ADE80",
];

/// Label dots, in the order the sidebar hands them out. Distinct from
/// `AVATARS`: a label is a flat spot of colour rather than a tinted disc, so
/// these are chosen to stay legible at 10px against the panel.
pub const LABEL_COLORS: &[&str] = &[
    "#EF4444", "#3B82F6", "#22C55E", "#A855F7", "#FACC15", "#F97316", "#EC4899", "#14B8A6",
];

/// Diameters avatars are drawn at. Each gets a rule in `css()`, because a
/// circle's radius has to track its size and GTK has no way to say "half of
/// whatever this is" in CSS.
pub const AVATAR_SIZES: &[i32] = &[20, 28, 36, 44, 56];

pub const RADIUS: u8 = 12;
pub const RADIUS_SMALL: u8 = 8;

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
.panel-raised {{ background-color: {PANEL_RAISED}; }}
.surface {{
    background-color: {SURFACE};
    border: 1px solid {BORDER};
    border-radius: {RADIUS}px;
}}

/* An HTML message: a white page, because that is what senders design for,
   with the same outline and corners as the plain-text card. */
.html-card {{
    background-color: #ffffff;
    border: 1px solid {BORDER};
    border-radius: {RADIUS}px;
}}
.images-bar {{ padding: 2px 4px; }}

/* The three mail columns are separated by a hairline rather than by GTK's
   default panel shadow, which is invisible at these values anyway. */
.column-edge {{ border-left: 1px solid {BORDER_SOFT}; }}

headerbar {{
    background-color: {BG_DEEP};
    box-shadow: none;
    border-bottom: 1px solid {BORDER_SOFT};
    min-height: 54px;
}}

/* Type scale. The egui UI sized text per label; here it is class-driven. */
.title {{ font-size: 17px; font-weight: 800; color: {TEXT}; }}
.subject {{ font-size: 22px; font-weight: 700; color: {TEXT}; }}
.heading-sm {{ font-size: 15px; font-weight: 700; color: {TEXT}; }}
.muted {{ color: {TEXT_MUTED}; }}
.faint {{ color: {TEXT_FAINT}; }}
.small {{ font-size: 11.5px; }}
.tiny {{ font-size: 10.5px; }}
.danger {{ color: {DANGER}; }}
.accent {{ color: {ACCENT}; }}
.section-label {{
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: {TEXT_FAINT};
}}

/* The search field in the header: one rounded well holding an icon, the
   entry itself and the shortcut hint, so the hint sits inside the pill. */
.searchbox {{
    background-color: {PANEL_RAISED};
    border: 1px solid {BORDER};
    border-radius: 11px;
    padding: 3px 10px;
}}
.searchbox:focus-within {{ border-color: {SELECTED_EDGE}; background-color: {SURFACE}; }}
.searchbox entry, .searchbox entry text {{
    background: none;
    background-image: none;
    border: none;
    box-shadow: none;
    outline: none;
    min-height: 28px;
    color: {TEXT};
}}
.searchbox image {{ color: {TEXT_FAINT}; }}

/* Keyboard hints — Ctrl K in the search well, Ctrl N on Compose. */
.kbd {{
    font-size: 10px;
    font-weight: 700;
    color: {TEXT_FAINT};
    background-color: {SURFACE_HOVER};
    border-radius: 5px;
    padding: 2px 6px;
}}
button.primary .kbd {{ background-color: alpha(#FFFFFF, 0.18); color: #FFFFFF; }}

/* Sidebar and message rows: the rounded highlight the painted rows had. */
.nav-row, .message-row {{
    border-radius: {RADIUS_SMALL}px;
    border: 1px solid transparent;
    background-color: transparent;
    padding: 5px 8px;
    min-height: 0;
}}
.nav-row:hover, .message-row:hover {{ background-color: {SURFACE_HOVER}; }}
.nav-row.selected {{
    background-color: {SURFACE};
    border-color: {BORDER};
}}
.nav-row.selected label {{ color: {TEXT}; }}
.nav-row.selected image {{ color: {ACCENT}; }}
.nav-row label {{ color: {TEXT_MUTED}; }}
.nav-row image {{ color: {TEXT_FAINT}; }}

/* Message list rows. The selected row is the one place the accent shows as a
   wash, which is what makes the open message obvious from across the pane. */
.message-row {{ border-radius: {RADIUS}px; padding: 10px 12px; }}
row.message-row:selected, .message-row.selected {{
    background-color: {SELECTED_BG};
    border-color: {SELECTED_EDGE};
}}
listview.messages {{ background: none; }}
listview.messages > row {{ padding: 0; background: none; border: none; }}
listview.messages > row:selected {{ background: none; }}
listview.messages > row:selected .message-row {{
    background-color: {SELECTED_BG};
    border-color: {SELECTED_EDGE};
}}
label.sender {{ font-size: 13.5px; font-weight: 600; color: {TEXT_MUTED}; }}
label.row-subject {{ font-size: 13px; color: {TEXT_MUTED}; }}
.unread label.sender {{ color: {TEXT}; font-weight: 800; }}
.unread label.row-subject {{ color: {TEXT}; font-weight: 600; }}
/* A starred row's mark, and the toolbar button once it is on. */
.star-on, button.star.on image {{ color: {STAR}; }}

/* Filter chips over the message list. */
.chip {{
    background: none;
    background-image: none;
    border: 1px solid transparent;
    border-radius: 9px;
    padding: 4px 12px;
    font-size: 12px;
    font-weight: 600;
    color: {TEXT_MUTED};
    min-height: 0;
    box-shadow: none;
}}
.chip:hover {{ background-color: {SURFACE_HOVER}; color: {TEXT}; }}
.chip.selected {{
    background-color: {SURFACE_ACTIVE};
    border-color: {SELECTED_EDGE};
    color: {TEXT};
}}

/* Counts, as on the sidebar's unread badges. */
.count-badge {{
    background-color: {SURFACE_HOVER};
    color: {TEXT_MUTED};
    border-radius: 9px;
    padding: 1px 7px;
    font-size: 11px;
    font-weight: 700;
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
    padding: 8px 12px;
    box-shadow: none;
}}
button.primary:hover {{ background-color: {ACCENT_HOVER}; }}
button.primary:active {{ background-color: {ACCENT_PRESSED}; }}
button.primary:disabled {{ background-color: {SURFACE_ACTIVE}; color: {TEXT_FAINT}; }}
button.destructive {{ background-image: none; background-color: {DANGER}; color: #FFFFFF; border: none; }}

/* Icon buttons: the reading-pane toolbar, the header, the contact actions. */
button.icon {{
    background: none;
    background-image: none;
    border: 1px solid transparent;
    border-radius: {RADIUS_SMALL}px;
    color: {TEXT_MUTED};
    min-width: 30px;
    min-height: 30px;
    padding: 4px;
    box-shadow: none;
}}
button.icon:hover {{ background-color: {SURFACE_HOVER}; color: {TEXT}; }}
button.icon:disabled {{ color: alpha({TEXT_FAINT}, 0.45); }}
button.icon.tile {{
    background-color: {SURFACE};
    border-color: {BORDER};
    min-width: 40px;
    min-height: 36px;
}}
button.icon.tile:hover {{ background-color: {SURFACE_HOVER}; border-color: {SELECTED_EDGE}; }}
.toolbar-row {{ border-bottom: 1px solid {BORDER_SOFT}; }}

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
    font-size: 13.5px;
}}
textview.compose, textview.compose text {{ background-color: {SURFACE}; color: {TEXT}; }}
.rule {{ background-color: {BORDER_SOFT}; min-height: 1px; }}
.statusbar {{ background-color: {BG_DEEP}; border-top: 1px solid {BORDER_SOFT}; }}
scrollbar {{ background: none; }}
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
            font = (f64::from(*size) * 0.42).round() as i32,
        ));
    }
    // One class per label dot.
    for (i, colour) in LABEL_COLORS.iter().enumerate() {
        css.push_str(&format!(
            ".label-dot-{i} {{ background-color: {colour}; border-radius: 5px; \
             min-width: 10px; min-height: 10px; }}\n"
        ));
    }
    css
}

/// Stable per-sender colour index, so the same correspondent keeps one dot
/// colour across restarts (no hashing of pointers or row ids).
pub fn avatar_color_index(seed: &str) -> usize {
    (hash(seed) as usize) % AVATARS.len()
}

/// The same trick for label dots, over the label palette.
pub fn label_color_index(seed: &str) -> usize {
    (hash(seed) as usize) % LABEL_COLORS.len()
}

fn hash(seed: &str) -> u32 {
    seed.trim()
        .to_ascii_lowercase()
        .bytes()
        .fold(0u32, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(u32::from(b))
        })
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

/// The coloured dot a label is listed with.
pub fn label_dot(name: &str) -> gtk::Box {
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class(&format!("label-dot-{}", label_color_index(name)));
    dot.set_valign(gtk::Align::Center);
    dot
}

/// A filled blue button for the one primary action in a view.
pub fn primary_button(text: &str) -> gtk::Button {
    let button = gtk::Button::with_label(text);
    button.add_css_class("primary");
    button
}

/// A flat, square-ish button carrying nothing but a symbolic icon — the
/// reading-pane toolbar, the header actions, the contact card's row.
pub fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.add_css_class("icon");
    button.set_tooltip_text(Some(tooltip));
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

/// A boxed keyboard hint, as in the search well and on the Compose button.
pub fn kbd(keys: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(keys));
    label.add_css_class("kbd");
    label.set_valign(gtk::Align::Center);
    label
}

/// A small uppercase heading over a group in the sidebar or contact pane.
pub fn section_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("section-label");
    label.set_xalign(0.0);
    label
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
