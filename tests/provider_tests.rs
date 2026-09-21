use airmail::models::SmtpSecurity;
use airmail::providers;

#[test]
fn known_domains_select_their_provider() {
    for (email, expected) in [
        ("someone@gmail.com", "Gmail"),
        ("SOMEONE@GoogleMail.com", "Gmail"),
        ("someone@hotmail.com", "Outlook"),
        ("someone@me.com", "iCloud"),
        ("someone@ymail.com", "Yahoo"),
    ] {
        let provider =
            providers::for_email(email).unwrap_or_else(|| panic!("no provider matched {email}"));
        assert_eq!(provider.name, expected, "for {email}");
    }
}

#[test]
fn unknown_domain_has_no_provider_but_still_guesses_servers() {
    assert!(providers::for_email("someone@example.invalid").is_none());

    let guess = providers::guess_config("someone@example.invalid");
    assert_eq!(guess.imap_host, "imap.example.invalid");
    assert_eq!(guess.smtp_host, "smtp.example.invalid");
    assert_eq!(guess.imap_port, 993);
    assert_eq!(guess.smtp_port, 465);
}

#[test]
fn malformed_addresses_are_rejected() {
    for bad in ["", "no-at-sign", "@nolocal.com", "trailing@", "two@@at.com"] {
        assert!(
            providers::domain_of(bad).is_none(),
            "{bad:?} should not parse"
        );
        assert!(
            providers::for_email(bad).is_none(),
            "{bad:?} should not match"
        );
    }
}

#[test]
fn provider_settings_reach_the_account_config() {
    let outlook = providers::for_email("someone@outlook.com").unwrap();
    let cfg = outlook.account_config("Someone@outlook.com ");

    assert_eq!(cfg.email, "Someone@outlook.com", "whitespace is trimmed");
    assert_eq!(cfg.imap_host, "outlook.office365.com");
    assert_eq!(cfg.smtp_port, 587);
    // Office 365 refuses implicit TLS; the old 587-over-SMTPS preset could not
    // have sent anything.
    assert_eq!(cfg.smtp_security, SmtpSecurity::StartTls);
}

#[test]
fn display_name_is_derived_from_the_address() {
    let cfg = providers::guess_config("ada.lovelace@example.invalid");
    assert_eq!(cfg.display_name.as_deref(), Some("Ada Lovelace"));

    // Nothing usable to derive from: better no name than a wrong one.
    let numeric = providers::guess_config("12345@example.invalid");
    assert_eq!(numeric.display_name, None);
}

#[test]
fn every_shipped_provider_is_usable() {
    for provider in providers::PROVIDERS {
        assert!(
            !provider.domains.is_empty(),
            "{} has no domains",
            provider.name
        );
        assert!(
            provider.imap_host.contains('.'),
            "{} imap host",
            provider.name
        );
        assert!(
            provider.smtp_host.contains('.'),
            "{} smtp host",
            provider.name
        );

        // Each domain must resolve back to this provider — no duplicates
        // shadowing each other.
        for domain in provider.domains {
            let matched = providers::for_email(&format!("user@{domain}"))
                .unwrap_or_else(|| panic!("{domain} matched nothing"));
            assert_eq!(
                matched.name, provider.name,
                "{domain} matched the wrong provider"
            );
        }
    }
}
