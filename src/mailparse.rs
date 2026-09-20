use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use mail_parser::{Address, DateTime as MailDate, Message};

#[derive(Debug, Clone, Default)]
pub struct ParsedMail {
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: Option<DateTime<Utc>>,
    pub body_text: String,
    pub body_html: String,
    pub has_attachments: bool,
}

pub fn parse(raw: &[u8]) -> Option<ParsedMail> {
    let msg: Message = raw.try_into().ok()?;
    Some(ParsedMail {
        subject: msg.subject().unwrap_or("").to_string(),
        from: msg.from().map(format_address).unwrap_or_default(),
        to: msg.to().map(format_address).unwrap_or_default(),
        date: msg.date().and_then(to_utc),
        body_text: msg.body_text(0).unwrap_or_default().to_string(),
        body_html: msg.body_html(0).unwrap_or_default().to_string(),
        has_attachments: msg.attachment_count() > 0,
    })
}

fn format_address(addr: &Address) -> String {
    match addr {
        Address::List(list) => list
            .iter()
            .map(|a| match (&a.name, &a.address) {
                (Some(name), Some(address)) => format!("{name} <{address}>"),
                (None, Some(address)) => address.to_string(),
                (Some(name), None) => name.to_string(),
                (None, None) => String::new(),
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Address::Group(groups) => groups
            .iter()
            .flat_map(|g| g.addresses.iter())
            .filter_map(|a| a.address.as_ref().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn to_utc(d: &MailDate) -> Option<DateTime<Utc>> {
    let date = NaiveDate::from_ymd_opt(i32::from(d.year), u32::from(d.month), u32::from(d.day))?;
    let time = NaiveTime::from_hms_opt(u32::from(d.hour), u32::from(d.minute), u32::from(d.second))?;
    let naive = NaiveDateTime::new(date, time);
    let offset = Duration::hours(i64::from(d.tz_hour)) + Duration::minutes(i64::from(d.tz_minute));
    // tz_before_gmt = true means the written time is behind GMT, so add the offset.
    let utc = if d.tz_before_gmt { naive + offset } else { naive - offset };
    Some(DateTime::<Utc>::from_naive_utc_and_offset(utc, Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/sample.msg");

    #[test]
    fn parses_fixture() {
        let parsed = parse(FIXTURE.as_bytes()).expect("fixture should parse");
        assert_eq!(parsed.subject, "AirMail fixture message");
        assert!(parsed.from.contains("alice@example.com"));
        assert!(parsed.to.contains("bob@example.com"));
        assert!(parsed.body_text.contains("Hello Bob"));
        assert!(!parsed.has_attachments);
        assert!(parsed.date.is_some());
    }

    #[test]
    fn parses_mime_multipart() {
        let raw = b"From: a@b.c\r\nTo: d@e.f\r\nSubject: multi\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"x\"\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nplain body\r\n--x\r\nContent-Type: text/html\r\n\r\n<p>html body</p>\r\n--x--\r\n";
        let parsed = parse(raw).expect("should parse");
        assert_eq!(parsed.body_text.trim(), "plain body");
        assert!(parsed.body_html.contains("html body"));
    }
}
