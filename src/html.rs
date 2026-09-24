//! Getting an HTML email ready for the reading pane's web view.
//!
//! A sender's HTML is untrusted, and the obvious harm is not scripts -- those
//! are off in the view -- but the network: a remote image is a read receipt,
//! and a remote stylesheet or font is the same thing in a different tag. So
//! every message goes in behind a Content-Security-Policy that lets it style
//! itself inline and load nothing from anywhere, and remote images are only
//! allowed when someone asks for them for that message.
//!
//! The policy is a `<meta>` placed ahead of everything the sender wrote. A
//! later policy in the message can only narrow it -- browsers enforce every
//! policy on a page, never the loosest one -- so nothing in the mail can undo
//! it. The web view carries a stricter floor of its own on top (see
//! `ui::html_view`), set where no document can reach it.

/// Let the message style itself and show images it carries inside it, and
/// load nothing else.
const POLICY_BLOCKED: &str = "default-src 'none'; style-src 'unsafe-inline'; \
     img-src data: cid:; font-src data:; media-src data:";

/// The same, with images from anywhere. Stylesheets and fonts stay local:
/// nobody asked for those, and they track just as well as an image does.
const POLICY_IMAGES: &str = "default-src 'none'; style-src 'unsafe-inline'; \
     img-src data: cid: https: http:; font-src data:; media-src data:";

/// What a message looks like before its own styles say otherwise. Senders
/// design for a white page, so it gets one whatever the app's theme is, and
/// images and preformatted text are kept inside the pane.
const BASE_STYLE: &str = "<style>\
     html{background:#fff;color:#1f2328;}\
     body{margin:16px;font:14px/1.5 system-ui,sans-serif;overflow-wrap:anywhere;}\
     img{max-width:100%;height:auto;}\
     pre{white-space:pre-wrap;}\
     </style>";

/// The document to hand the web view: the policy and base styles, then the
/// message exactly as it came.
pub fn prepare(body_html: &str, allow_remote_images: bool) -> String {
    let policy = if allow_remote_images {
        POLICY_IMAGES
    } else {
        POLICY_BLOCKED
    };
    format!(
        "<meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{policy}\">\
         <meta name=\"viewport\" content=\"width=device-width\">\
         {BASE_STYLE}{body_html}"
    )
}

/// Whether the message refers to anything on the network that the policy
/// would block, so the pane knows to offer loading it. A plain search rather
/// than a parse: a false positive only costs a button nobody needed.
pub fn has_remote_content(body_html: &str) -> bool {
    let lower = body_html.to_ascii_lowercase();
    let squeezed: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
    [
        "src=\"http",
        "src='http",
        "src=http",
        "srcset=\"http",
        "srcset='http",
        "background=\"http",
        "background='http",
        "url(http",
        "url(\"http",
        "url('http",
    ]
    .iter()
    .any(|needle| squeezed.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_comes_before_anything_the_sender_wrote() {
        let doc = prepare(
            "<meta http-equiv=\"Content-Security-Policy\" content=\"img-src *\">",
            false,
        );
        let ours = doc.find(POLICY_BLOCKED).expect("policy present");
        let theirs = doc.find("img-src *").unwrap();
        assert!(ours < theirs);
    }

    #[test]
    fn remote_images_are_blocked_until_asked_for() {
        assert!(!prepare("", false).contains("https:"));
        assert!(prepare("", true).contains("img-src data: cid: https: http:"));
    }

    #[test]
    fn remote_content_is_noticed_in_its_usual_places() {
        assert!(has_remote_content(
            "<img src=\"https://t.example/pixel.gif\">"
        ));
        assert!(has_remote_content("<IMG SRC = 'http://x'>"));
        assert!(has_remote_content(
            "<td style=\"background: url( https://x/bg.png )\">"
        ));
        assert!(has_remote_content(
            "<table background=\"https://x/bg.png\">"
        ));
        assert!(!has_remote_content(
            "<img src=\"cid:logo@x\"><a href=\"https://x\">link</a>"
        ));
    }
}
