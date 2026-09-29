//! `mailto:` links (RFC 6068), which is how the rest of the desktop asks the
//! default mail client to write a message: a link in a browser, "Email" in a
//! file manager, `raven-open mailto:ada@example.com`.
//!
//! Only what the composer has a field for is read -- recipients, subject and
//! body. `cc`, `bcc` and the rest are dropped rather than folded into `to`,
//! because sending someone a copy they were meant not to see is worse than
//! leaving them off.

/// The parts of a `mailto:` link the composer can use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mailto {
    /// Every recipient, from the path and any `to=` fields, `, `-separated.
    pub to: String,
    pub subject: String,
    pub body: String,
}

impl Mailto {
    /// Parse a `mailto:` URI, or `None` if `uri` is not one. The scheme is
    /// matched without regard to case, as URI schemes are.
    pub fn parse(uri: &str) -> Option<Self> {
        let scheme = uri.get(..7)?;
        if !scheme.eq_ignore_ascii_case("mailto:") {
            return None;
        }
        let rest = &uri[7..];
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));

        let mut to: Vec<String> = split_addresses(&decode(path));
        let mut out = Self::default();
        for field in query.split('&').filter(|f| !f.is_empty()) {
            let (key, value) = field.split_once('=').unwrap_or((field, ""));
            let value = decode(value);
            match decode(key).to_ascii_lowercase().as_str() {
                "to" => to.extend(split_addresses(&value)),
                "subject" => out.subject = value,
                // RFC 6068 wants line breaks as %0D%0A; the composer is a
                // plain GTK text view and wants them as \n.
                "body" => out.body = value.replace("\r\n", "\n"),
                _ => {}
            }
        }
        out.to = to.join(", ");
        Some(out)
    }
}

fn split_addresses(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Percent-decoding, and only that: in a `mailto:` link `+` is a plus sign,
/// not a space, so this is not form decoding. A malformed escape is kept as
/// it was written.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_address() {
        let m = Mailto::parse("mailto:ada@example.com").unwrap();
        assert_eq!(m.to, "ada@example.com");
        assert!(m.subject.is_empty() && m.body.is_empty());
    }

    #[test]
    fn subject_and_body_are_decoded() {
        let m = Mailto::parse(
            "MAILTO:ada@example.com?subject=Hello%20there&body=Line%201%0D%0ALine%202",
        )
        .unwrap();
        assert_eq!(m.subject, "Hello there");
        assert_eq!(m.body, "Line 1\nLine 2");
    }

    #[test]
    fn plus_is_a_plus() {
        let m = Mailto::parse("mailto:ada+lists@example.com?subject=1+1").unwrap();
        assert_eq!(m.to, "ada+lists@example.com");
        assert_eq!(m.subject, "1+1");
    }

    #[test]
    fn recipients_from_path_and_to_fields_are_joined() {
        let m = Mailto::parse("mailto:a@x.org,b@x.org?to=c@x.org&cc=d@x.org").unwrap();
        assert_eq!(m.to, "a@x.org, b@x.org, c@x.org");
    }

    #[test]
    fn no_address_is_fine() {
        let m = Mailto::parse("mailto:?subject=Hi").unwrap();
        assert_eq!(m.to, "");
        assert_eq!(m.subject, "Hi");
    }

    #[test]
    fn a_malformed_escape_is_kept() {
        assert_eq!(
            Mailto::parse("mailto:?subject=100%").unwrap().subject,
            "100%"
        );
        assert_eq!(Mailto::parse("mailto:?subject=%zz").unwrap().subject, "%zz");
    }

    #[test]
    fn other_schemes_are_not_mailto() {
        assert!(Mailto::parse("https://example.com").is_none());
        assert!(Mailto::parse("mail").is_none());
    }
}
