//! Exercises the decisions behind the account dialog and the palette helpers.
//!
//! The GTK UI cannot be instantiated without a display, so the parts worth
//! testing are the ones that hold no widgets: `SetupForm`, which decides what
//! an address implies and what it saves, and the pure helpers in `theme`.

use airmail::models::SmtpSecurity;
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
