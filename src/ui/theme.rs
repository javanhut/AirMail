//! The Raven palette and the handful of widgets built on top of it.
//!
//! Everything visual lives here so the panels stay about mail: the rest of the
//! UI asks for a `.surface` style class or `theme::avatar()` rather than
//! mixing its own colours. Under GTK the palette is a stylesheet rather than a
//! set of paint calls, so the palettes below exist to be interpolated into
//! `css_for()` — which is a pure function, and therefore testable without a
//! display.
//!
//! Light or dark, the accent, the glass theme and window transparency are
//! the desktop's, read from `~/.config/raven/desktop.toml` (see
//! `crate::desktop`) and followed live: when Raven Settings rewrites the file
//! the stylesheet is rebuilt and swapped in place.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;

use crate::desktop::{self, Desktop, ThemeMode};

/// The neutrals one scheme is drawn in. The accent is not here: it is the
/// desktop's, and every accent tint below (hover, pressed, the selected-row
/// wash and its edge) is derived from it in `css_for()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    // Backgrounds, deepest first. The window sits on the ground; the three
    // mail columns step up from it, and cards step up again toward the reader.
    pub bg_deep: &'static str,
    pub panel: &'static str,
    pub panel_raised: &'static str,
    pub surface: &'static str,
    pub surface_hover: &'static str,
    pub surface_active: &'static str,

    pub border: &'static str,
    pub border_soft: &'static str,

    pub text: &'static str,
    pub text_muted: &'static str,
    pub text_faint: &'static str,

    pub danger: &'static str,
    pub warn: &'static str,
    pub success: &'static str,
    pub star: &'static str,

    /// How much of the accent the selected row's wash and edge carry, mixed
    /// into `surface`. A dark ground needs less to read as "lit".
    pub selected_wash: f64,
    pub selected_edge: f64,
    /// `shade()` factor for an avatar's initial: full strength on dark,
    /// darkened on light so a yellow initial stays legible on its pale disc.
    pub avatar_ink: f64,
}

/// AirMail's own look: near-black navy.
pub const DARK: Palette = Palette {
    bg_deep: "#070A12",
    panel: "#0C111C",
    panel_raised: "#0F1524",
    surface: "#141B2B",
    surface_hover: "#1A2234",
    surface_active: "#222C44",
    border: "#1C2435",
    border_soft: "#151C2A",
    text: "#E8ECF4",
    text_muted: "#98A2B8",
    text_faint: "#69748C",
    danger: "#F87171",
    warn: "#FBBF24",
    success: "#4ADE80",
    star: "#FBBF24",
    selected_wash: 0.25,
    selected_edge: 0.60,
    avatar_ink: 1.0,
};

/// The same hierarchy on paper: a cool grey ground, white columns and cards,
/// ink-dark text, and status colours a step deeper so they hold on white.
pub const LIGHT: Palette = Palette {
    bg_deep: "#E9EDF4",
    panel: "#F4F6FA",
    panel_raised: "#FAFBFD",
    surface: "#FFFFFF",
    surface_hover: "#E6EAF1",
    surface_active: "#D8DFEA",
    border: "#D3DAE5",
    border_soft: "#DFE4EC",
    text: "#141A26",
    text_muted: "#4A5468",
    text_faint: "#7A8499",
    danger: "#DC2626",
    warn: "#B7791F",
    success: "#16A34A",
    star: "#D69E00",
    selected_wash: 0.14,
    selected_edge: 0.55,
    avatar_ink: 0.62,
};

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

/// Diameters avatars are drawn at. Each gets a rule in `css_for()`, because a
/// circle's radius has to track its size and GTK has no way to say "half of
/// whatever this is" in CSS.
pub const AVATAR_SIZES: &[i32] = &[20, 28, 36, 44, 56];

pub const RADIUS: u8 = 12;
pub const RADIUS_SMALL: u8 = 8;

/// How long the desktop file has to stay quiet before it is re-read. Settings
/// writes it by rename, which a directory monitor reports as a burst.
const DESKTOP_SETTLE: Duration = Duration::from_millis(150);

thread_local! {
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static DESKTOP_MONITOR: RefCell<Option<gtk::gio::FileMonitor>> = const { RefCell::new(None) };
}

/// Install the stylesheet for the desktop's current appearance and start
/// following `desktop.toml`. Called at startup; safe to call again.
pub fn apply() {
    apply_desktop(&Desktop::load());
    watch_desktop();
}

/// Set libadwaita's scheme and swap in the stylesheet for `desktop`. The
/// previous provider is removed, never stacked, so this can run on every
/// change of the file.
pub fn apply_desktop(desktop: &Desktop) {
    let appearance = &desktop.appearance;
    adw::StyleManager::default().set_color_scheme(match appearance.theme_mode {
        ThemeMode::Dark => adw::ColorScheme::ForceDark,
        ThemeMode::Light => adw::ColorScheme::ForceLight,
        // Auto is dark across Raven; libadwaita may still follow a portal.
        ThemeMode::Auto => adw::ColorScheme::PreferDark,
    });

    let css = css_tinted(
        palette_for(appearance.theme_mode),
        desktop.accent(),
        appearance.transparency,
        Tint::for_glass(
            &appearance.glass_theme,
            appearance.theme_mode == ThemeMode::Light,
        ),
    );
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    PROVIDER.with(|slot| {
        if let Some(old) = slot.borrow_mut().take() {
            gtk::style_context_remove_provider_for_display(&display, &old);
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&css);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        *slot.borrow_mut() = Some(provider);
    });
}

/// Light is light; dark and auto are dark (Raven's auto means dark).
pub fn palette_for(mode: ThemeMode) -> &'static Palette {
    match mode {
        ThemeMode::Light => &LIGHT,
        ThemeMode::Dark | ThemeMode::Auto => &DARK,
    }
}

/// A glass theme other than Black: its ground and text, which AirMail's
/// neutrals are re-drawn between (see `tone`). Black Glass is no tint at
/// all, so it draws exactly the palettes above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint {
    ground: [u8; 3],
    text: [u8; 3],
}

impl Tint {
    /// The tint for `appearance.glass_theme`, or `None` for Black Glass (and
    /// for a theme this build does not know). The colours are
    /// `raven_glass::tint`'s, so AirMail wears the compositor's grounds.
    pub fn for_glass(theme: &str, light: bool) -> Option<Tint> {
        let css = raven_glass::tint::css(theme, light);
        let colour = |name: &str| {
            let at = css.find(&format!("@define-color {name} #"))? + name.len() + 16;
            rgb(css.get(at..at + 6)?)
        };
        Some(Tint {
            ground: colour("window_bg_color")?,
            text: colour("window_fg_color")?,
        })
    }

    /// `colour` moved from `p`'s ground-to-text axis onto this tint's: a
    /// surface a shade off the ground stays a shade off the new ground, muted
    /// text stays as far between ground and text as it was.
    fn tone(&self, p: &Palette, colour: &str) -> String {
        let (Some(c), Some(g), Some(t)) = (rgb(colour), rgb(p.bg_deep), rgb(p.text)) else {
            return colour.to_string();
        };
        let axis = |i: usize| f64::from(t[i]) - f64::from(g[i]);
        let along: f64 = (0..3)
            .map(|i| (f64::from(c[i]) - f64::from(g[i])) * axis(i))
            .sum();
        let length: f64 = (0..3).map(|i| axis(i) * axis(i)).sum();
        let k = if length > 0.0 { along / length } else { 0.0 };
        let m = |i: usize| {
            let (a, b) = (f64::from(self.ground[i]), f64::from(self.text[i]));
            (a + (b - a) * k).round().clamp(0.0, 255.0) as u8
        };
        format!("#{:02X}{:02X}{:02X}", m(0), m(1), m(2))
    }
}

fn rgb(hex: &str) -> Option<[u8; 3]> {
    let h = hex.strip_prefix('#').unwrap_or(hex);
    let at = |i: usize| h.get(i..i + 2).and_then(|c| u8::from_str_radix(c, 16).ok());
    Some([at(0)?, at(2)?, at(4)?])
}

/// Re-apply whenever Settings rewrites `desktop.toml`. The directory is
/// watched rather than the file, because the file may not exist yet and is
/// replaced by rename; events are filtered by name and debounced.
fn watch_desktop() {
    if DESKTOP_MONITOR.with(|m| m.borrow().is_some()) {
        return;
    }
    let path = desktop::path();
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let name = name.to_os_string();
    let monitor = match gtk::gio::File::for_path(dir).monitor_directory(
        gtk::gio::FileMonitorFlags::WATCH_MOVES,
        gtk::gio::Cancellable::NONE,
    ) {
        Ok(monitor) => monitor,
        Err(e) => {
            tracing::debug!("not following {}: {e}", path.display());
            return;
        }
    };
    let pending: Rc<RefCell<Option<gtk::glib::SourceId>>> = Rc::new(RefCell::new(None));
    monitor.connect_changed(move |_, file, other, event| {
        if matches!(
            event,
            gtk::gio::FileMonitorEvent::AttributeChanged
                | gtk::gio::FileMonitorEvent::PreUnmount
                | gtk::gio::FileMonitorEvent::Unmounted
        ) {
            return;
        }
        let names_desktop = |f: Option<&gtk::gio::File>| {
            f.and_then(|f| f.basename())
                .is_some_and(|b| b.as_os_str() == name.as_os_str())
        };
        if !names_desktop(Some(file)) && !names_desktop(other) {
            return;
        }
        if let Some(id) = pending.borrow_mut().take() {
            id.remove();
        }
        let fired = pending.clone();
        let id = gtk::glib::timeout_add_local_once(DESKTOP_SETTLE, move || {
            fired.borrow_mut().take();
            apply_desktop(&Desktop::load());
        });
        *pending.borrow_mut() = Some(id);
    });
    DESKTOP_MONITOR.with(|m| *m.borrow_mut() = Some(monitor));
}

/// The stylesheet as the defaults draw it: dark, Raven's accent, glass on.
pub fn css() -> String {
    css_for(&DARK, desktop::DEFAULT_ACCENT, true)
}

/// The whole stylesheet, as a string. Built here rather than shipped as a
/// static `.css` file so the palettes stay the single source of truth, and so
/// a test can check it without opening a display.
///
/// `accent` must be `#RRGGBB` (`Desktop::accent()` guarantees it). With
/// `glass`, the window ground and the mail columns let some of the desktop
/// through; the compositor draws the blur. Cards and the reader stay opaque.
pub fn css_for(p: &Palette, accent: &str, glass: bool) -> String {
    css_tinted(p, accent, glass, None)
}

/// [`css_for`], with the neutrals re-drawn in a glass theme's ground and
/// text. The accent and the status colours stay as they are.
pub fn css_tinted(p: &Palette, accent: &str, glass: bool, tint: Option<Tint>) -> String {
    let Palette { danger, star, .. } = *p;
    let tone = |colour: &str| match tint {
        Some(tint) => tint.tone(p, colour),
        None => colour.to_string(),
    };
    let panel_raised = tone(p.panel_raised);
    let surface = tone(p.surface);
    let surface_hover = tone(p.surface_hover);
    let surface_active = tone(p.surface_active);
    let border = tone(p.border);
    let border_soft = tone(p.border_soft);
    let text = tone(p.text);
    let text_muted = tone(p.text_muted);
    let text_faint = tone(p.text_faint);
    let (bg_deep, panel) = if glass {
        (
            format!("alpha({}, 0.86)", tone(p.bg_deep)),
            format!("alpha({}, 0.70)", tone(p.panel)),
        )
    } else {
        (tone(p.bg_deep), tone(p.panel))
    };
    let accent_hover = format!("shade({accent}, 1.15)");
    let accent_pressed = format!("shade({accent}, 0.85)");
    let selected_bg = format!("mix({surface}, {accent}, {})", p.selected_wash);
    let selected_edge = format!("mix({surface}, {accent}, {})", p.selected_edge);
    // libadwaita's own widgets (switches, focus rings, suggested buttons)
    // take the desktop's accent too.
    let mut css =
        format!("@define-color accent_bg_color {accent};\n@define-color accent_color {accent};\n");
    css.push_str(&format!(
        "
window.airmail, .bg-deep {{ background-color: {bg_deep}; color: {text}; }}
.panel {{ background-color: {panel}; }}
.panel-raised {{ background-color: {panel_raised}; }}
.surface {{
    background-color: {surface};
    border: 1px solid {border};
    border-radius: {RADIUS}px;
}}

/* An HTML message: a white page, because that is what senders design for,
   with the same outline and corners as the plain-text card. */
.html-card {{
    background-color: #ffffff;
    border: 1px solid {border};
    border-radius: {RADIUS}px;
}}
.images-bar {{ padding: 2px 4px; }}

/* The three mail columns are separated by a hairline rather than by GTK's
   default panel shadow, which is invisible at these values anyway. */
.column-edge {{ border-left: 1px solid {border_soft}; }}

headerbar {{
    background-color: {bg_deep};
    box-shadow: none;
    border-bottom: 1px solid {border_soft};
    min-height: 54px;
}}

/* Type scale. The egui UI sized text per label; here it is class-driven. */
.title {{ font-size: 17px; font-weight: 800; color: {text}; }}
.subject {{ font-size: 22px; font-weight: 700; color: {text}; }}
.heading-sm {{ font-size: 15px; font-weight: 700; color: {text}; }}
.muted {{ color: {text_muted}; }}
.faint {{ color: {text_faint}; }}
.small {{ font-size: 11.5px; }}
.tiny {{ font-size: 10.5px; }}
.danger {{ color: {danger}; }}
.accent {{ color: {accent}; }}
.section-label {{
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: {text_faint};
}}

/* The search field in the header: one rounded well holding an icon, the
   entry itself and the shortcut hint, so the hint sits inside the pill. */
.searchbox {{
    background-color: {panel_raised};
    border: 1px solid {border};
    border-radius: 11px;
    padding: 3px 10px;
}}
.searchbox:focus-within {{ border-color: {selected_edge}; background-color: {surface}; }}
.searchbox entry, .searchbox entry text {{
    background: none;
    background-image: none;
    border: none;
    box-shadow: none;
    outline: none;
    min-height: 28px;
    color: {text};
}}
.searchbox image {{ color: {text_faint}; }}

/* Keyboard hints — Ctrl K in the search well, Ctrl N on Compose. */
.kbd {{
    font-size: 10px;
    font-weight: 700;
    color: {text_faint};
    background-color: {surface_hover};
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
.nav-row:hover, .message-row:hover {{ background-color: {surface_hover}; }}
.nav-row.selected {{
    background-color: {surface};
    border-color: {border};
}}
.nav-row.selected label {{ color: {text}; }}
.nav-row.selected image {{ color: {accent}; }}
.nav-row label {{ color: {text_muted}; }}
.nav-row image {{ color: {text_faint}; }}

/* Message list rows. The selected row is the one place the accent shows as a
   wash, which is what makes the open message obvious from across the pane. */
.message-row {{ border-radius: {RADIUS}px; padding: 10px 12px; }}
row.message-row:selected, .message-row.selected {{
    background-color: {selected_bg};
    border-color: {selected_edge};
}}
listview.messages {{ background: none; }}
listview.messages > row {{ padding: 0; background: none; border: none; }}
listview.messages > row:selected {{ background: none; }}
listview.messages > row:selected .message-row {{
    background-color: {selected_bg};
    border-color: {selected_edge};
}}
label.sender {{ font-size: 13.5px; font-weight: 600; color: {text_muted}; }}
label.row-subject {{ font-size: 13px; color: {text_muted}; }}
.unread label.sender {{ color: {text}; font-weight: 800; }}
.unread label.row-subject {{ color: {text}; font-weight: 600; }}
/* A starred row's mark, and the toolbar button once it is on. */
.star-on, button.star.on image {{ color: {star}; }}

/* Filter chips over the message list. */
.chip {{
    background: none;
    background-image: none;
    border: 1px solid transparent;
    border-radius: 9px;
    padding: 4px 12px;
    font-size: 12px;
    font-weight: 600;
    color: {text_muted};
    min-height: 0;
    box-shadow: none;
}}
.chip:hover {{ background-color: {surface_hover}; color: {text}; }}
.chip.selected {{
    background-color: {surface_active};
    border-color: {selected_edge};
    color: {text};
}}

/* Counts, as on the sidebar's unread badges. */
.count-badge {{
    background-color: {surface_hover};
    color: {text_muted};
    border-radius: 9px;
    padding: 1px 7px;
    font-size: 11px;
    font-weight: 700;
}}
.count-badge.highlight {{ background-color: {accent}; color: #FFFFFF; }}

/* The one primary action in a view. */
button.primary {{
    background-image: none;
    background-color: {accent};
    color: #FFFFFF;
    font-weight: 700;
    border: none;
    border-radius: {RADIUS_SMALL}px;
    padding: 8px 12px;
    box-shadow: none;
}}
button.primary:hover {{ background-color: {accent_hover}; }}
button.primary:active {{ background-color: {accent_pressed}; }}
button.primary:disabled {{ background-color: {surface_active}; color: {text_faint}; }}
button.destructive {{ background-image: none; background-color: {danger}; color: #FFFFFF; border: none; }}

/* Icon buttons: the reading-pane toolbar, the header, the contact actions. */
button.icon {{
    background: none;
    background-image: none;
    border: 1px solid transparent;
    border-radius: {RADIUS_SMALL}px;
    color: {text_muted};
    min-width: 30px;
    min-height: 30px;
    padding: 4px;
    box-shadow: none;
}}
button.icon:hover {{ background-color: {surface_hover}; color: {text}; }}
button.icon:disabled {{ color: alpha({text_faint}, 0.45); }}
button.icon.tile {{
    background-color: {surface};
    border-color: {border};
    min-width: 40px;
    min-height: 36px;
}}
button.icon.tile:hover {{ background-color: {surface_hover}; border-color: {selected_edge}; }}
.toolbar-row {{ border-bottom: 1px solid {border_soft}; }}

/* Provider tiles in the setup dialog. */
.tile {{
    background-color: {surface};
    border: 1px solid {border};
    border-radius: {RADIUS}px;
    padding: 10px 12px;
    color: {text_muted};
}}
.tile:hover {{ background-color: {surface_hover}; border-color: {selected_edge}; color: {text}; }}
.tile:checked {{ background-color: {selected_bg}; border-color: {accent}; color: {text}; }}

textview.reading, textview.reading text {{
    background-color: {surface};
    color: {text};
    font-size: 13.5px;
}}
textview.compose, textview.compose text {{ background-color: {surface}; color: {text}; }}
.rule {{ background-color: {border_soft}; min-height: 1px; }}
.statusbar {{ background-color: {bg_deep}; border-top: 1px solid {border_soft}; }}
scrollbar {{ background: none; }}
"
    ));

    // One class per avatar colour: a tinted disc with the sender's initial,
    // matching the fill/stroke/text weights the painted version used.
    for (i, colour) in AVATARS.iter().enumerate() {
        css.push_str(&format!(
            ".avatar-{i} {{ background-color: alpha({colour}, 0.30); \
             border: 1px solid alpha({colour}, 0.75); color: shade({colour}, {ink}); }}\n",
            ink = p.avatar_ink,
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
