//! Exercises the decisions behind the account dialog and the palette helpers.
//!
//! The GTK UI cannot be instantiated without a display, so the parts worth
//! testing are the ones that hold no widgets: `SetupForm`, which decides what
//! an address implies and what it saves, `NewKeyring`, which decides whether
//! two typed passwords are worth sending, and the pure helpers in `theme`.

use airmail::models::{OAuthProvider, SmtpSecurity};
use airmail::ui::keyring_setup::NewKeyring;
use airmail::ui::setup::{Choice, SetupForm};
use airmail::ui::theme;

#[test]
fn a_fresh_form_has_nothing_to_save() {
    let form = SetupForm::default();
    assert!(form.choice().is_none());
    assert!(!form.is_ready(), "nothing typed yet");
    assert!(form.validate().is_err());
}

#[test]
fn a_known_address_picks_its_provider() {
    let mut form = SetupForm::default();
    form.set_email("ada@gmail.com");
    match form.choice() {
        Some(Choice::Known(provider)) => assert_eq!(provider.name, "Gmail"),
        other => panic!("expected Gmail, got {other:?}"),
    }
    assert!(form.hint().is_some(), "Gmail wants an app password");

    form.set_password("app-password");
    assert!(form.is_ready());
    let (config, password) = form
        .save()
        .expect("a known provider needs no server fields");
    assert_eq!(config.email, "ada@gmail.com");
    assert_eq!(config.imap_host, "imap.gmail.com");
    assert_eq!(password, "app-password");
}

#[test]
fn gmail_signs_in_through_the_browser_when_a_client_is_configured() {
    let mut form = SetupForm::default().with_browser_sign_in(vec![OAuthProvider::Google]);
    form.set_email("ada@gmail.com");
    assert_eq!(form.browser_provider(), Some(OAuthProvider::Google));
    assert!(form.is_ready(), "no password needed");

    let (config, password) = form.save().expect("nothing else to fill in");
    assert_eq!(config.oauth, Some(OAuthProvider::Google));
    assert!(password.is_empty(), "the refresh token comes later");
}

#[test]
fn the_app_password_hint_explains_how_to_set_up_browser_sign_in() {
    for (address, section) in [
        ("ada@gmail.com", "[google]"),
        ("ada@outlook.com", "[microsoft]"),
    ] {
        let mut form = SetupForm::default();
        form.set_email(address);
        let hint = form
            .hint()
            .expect("both want something other than the web password");
        assert!(hint.contains("app password"), "{address}: {hint}");
        assert!(
            hint.contains("oauth.toml") && hint.contains(section),
            "{address}: {hint}"
        );
        // Broken markup would leave the label blank. Links are GtkLabel's
        // addition to Pango markup, so they come out before Pango checks it.
        gtk::pango::parse_markup(&without_links(&hint), '\0').expect("hint is valid markup");
    }

    // A provider with no browser sign-in gets only the password hint.
    let mut form = SetupForm::default();
    form.set_email("ada@icloud.com");
    assert!(!form.hint().unwrap().contains("oauth.toml"));
}

#[test]
fn providers_without_browser_sign_in_still_want_a_password() {
    let mut form = SetupForm::default().with_browser_sign_in(vec![OAuthProvider::Google]);
    form.set_email("ada@icloud.com");
    assert_eq!(form.browser_provider(), None);
    assert!(!form.is_ready());

    // Outlook offers it, but this machine has no Microsoft client ID.
    form.set_email("ada@outlook.com");
    assert_eq!(form.browser_provider(), None);
}

#[test]
fn an_unknown_domain_falls_back_to_guessed_servers() {
    let mut form = SetupForm::default();
    form.set_email("ada@lovelace.dev");
    assert_eq!(form.choice(), Some(Choice::Other));

    let custom = form.custom();
    assert!(custom.derived, "the fields still hold guesses");
    assert_eq!(custom.imap_host, "imap.lovelace.dev");
    assert_eq!(custom.smtp_host, "smtp.lovelace.dev");

    form.set_password("hunter2");
    let (config, _) = form.save().expect("guessed servers are enough to save");
    assert_eq!(config.imap_host, "imap.lovelace.dev");
    assert_eq!(config.smtp_host, "smtp.lovelace.dev");
}

#[test]
fn typed_servers_survive_further_typing_in_the_address() {
    let mut form = SetupForm::default();
    form.set_email("ada@lovelace.dev");
    form.set_custom(|custom| custom.imap_host = "mail.example.net".to_string());
    // Still typing the address must not overwrite what was entered by hand.
    form.set_email("ada@lovelace.development");
    assert_eq!(form.custom().imap_host, "mail.example.net");
    assert!(!form.custom().derived);

    form.set_password("hunter2");
    let (config, _) = form.save().expect("typed servers are valid");
    assert_eq!(config.imap_host, "mail.example.net");
}

#[test]
fn a_hand_picked_provider_is_not_moved_by_the_address() {
    let mut form = SetupForm::default();
    form.choose(Choice::Other);
    form.set_email("ada@gmail.com");
    assert_eq!(
        form.choice(),
        Some(Choice::Other),
        "the pointer should not move out from under the click"
    );
}

#[test]
fn bad_input_is_reported_rather_than_saved() {
    let mut form = SetupForm::default();
    form.set_email("not-an-address");
    form.set_password("hunter2");
    assert!(form.validate().is_err(), "an address needs a domain");

    let mut form = SetupForm::default();
    form.set_email("ada@lovelace.dev");
    assert!(form.validate().is_err(), "a password is required");

    let mut form = SetupForm::default();
    form.set_email("ada@lovelace.dev");
    form.set_password("hunter2");
    form.set_custom(|custom| custom.imap_port = "half past four".to_string());
    let error = form.validate().expect_err("that is not a port");
    assert!(format!("{error:#}").contains("IMAP port"));
}

#[test]
fn encryption_choice_reaches_the_saved_account() {
    let mut form = SetupForm::default();
    form.set_email("ada@lovelace.dev");
    form.set_password("hunter2");
    form.set_custom(|custom| custom.smtp_security = SmtpSecurity::StartTls);
    let (config, _) = form.save().unwrap();
    assert_eq!(config.smtp_security, SmtpSecurity::StartTls);
}

#[test]
fn avatar_initials_cope_with_awkward_names() {
    assert_eq!(theme::initial("Ada Lovelace"), "A");
    assert_eq!(theme::initial("\"Ada\" <ada@x.y>"), "A");
    assert_eq!(theme::initial("<ada@x.y>"), "A");
    assert_eq!(theme::initial("  "), "?");
    assert_eq!(theme::initial(""), "?");
    assert_eq!(theme::initial("…!"), "?");
}

#[test]
fn avatar_colour_is_stable_and_case_insensitive() {
    assert_eq!(
        theme::avatar_color("ada@example.com"),
        theme::avatar_color("ADA@Example.com "),
    );
}

#[test]
fn the_stylesheet_covers_every_avatar_colour() {
    let css = theme::css();
    for index in 0..theme::AVATARS.len() {
        assert!(
            css.contains(&format!(".avatar-{index}")),
            "no rule for avatar colour {index}"
        );
    }
    // A stray `{}` from a format string would take the whole sheet down.
    assert_eq!(
        css.matches('{').count(),
        css.matches('}').count(),
        "unbalanced braces in the stylesheet"
    );
}

#[test]
fn a_new_keyring_needs_both_fields() {
    let empty = NewKeyring::default();
    assert!(!empty.is_complete(), "nothing typed yet");
    assert!(empty.validate().is_err());

    let half_typed = NewKeyring {
        password: "hunter2".into(),
        again: String::new(),
    };
    assert!(!half_typed.is_complete(), "the second field is still empty");
}

#[test]
fn a_new_keyring_says_when_the_two_disagree() {
    let mut form = NewKeyring {
        password: "hunter2".into(),
        again: "hunter3".into(),
    };

    // Complete, so the button is live -- the complaint belongs in a sentence,
    // not in a button that greys itself out while the user is still typing.
    assert!(form.is_complete());
    assert_eq!(form.validate(), Err("Those two passwords do not match."));

    form.again = "hunter2".into();
    assert_eq!(form.validate(), Ok("hunter2"));
}

/// Markup with `<a href="…">` and `</a>` removed, keeping the link text.
fn without_links(markup: &str) -> String {
    let mut out = markup.replace("</a>", "");
    while let Some(start) = out.find("<a ") {
        let end = start + out[start..].find('>').expect("unclosed <a> tag");
        out.replace_range(start..=end, "");
    }
    out
}
