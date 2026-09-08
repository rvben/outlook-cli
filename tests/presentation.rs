use outlook_cli::{
    graph::Page,
    presentation::{PageKind, message_text, page_text, safe_text},
};
use serde_json::json;

#[test]
fn mail_keeps_full_subject_and_actionable_id_without_terminal_controls() {
    let subject = "A long subject that should remain readable without losing its ending 東京";
    let page = Page {
        items: vec![json!({"id":"copy-this-id", "subject":subject,
        "from":{"emailAddress":{"name":"Sender\nInjected\u{001b}[2J", "address":"sender@example.com"}},
        "isRead":false,"hasAttachments":true})],
        next_cursor: None,
        truncated: false,
    };
    let text = page_text(&page, PageKind::Messages);
    assert!(text.contains(subject));
    assert!(text.contains("copy-this-id"));
    assert!(text.contains("Sender Injected <sender@example.com>"));
    assert!(text.contains("unread · attachments"));
    assert!(!text.contains('\x1b'));
}

#[test]
fn empty_search_page_can_still_have_more_results() {
    let page = Page {
        items: vec![],
        next_cursor: Some("next-page".into()),
        truncated: true,
    };
    let text = page_text(&page, PageKind::Messages);
    assert!(text.contains("No messages in this page."));
    assert!(text.contains("--cursor 'next-page'"));
    assert!(!text.contains("JSON"));
}

#[test]
fn projected_fields_do_not_invent_counts_or_status() {
    let page = Page {
        items: vec![json!({"id":"one"})],
        next_cursor: None,
        truncated: false,
    };
    let text = page_text(&page, PageKind::Folders);
    assert!(text.contains("one"));
    assert!(!text.contains("0 unread"));
    let text = page_text(&page, PageKind::Messages);
    assert!(!text.contains("unread"));
}

#[test]
fn reading_shows_recipients_and_preserves_body_paragraphs() {
    let text = message_text(
        &json!({"subject":"Hello", "toRecipients":[{"emailAddress":{"address":"reader@example.com"}}],
        "body":{"content":"First\r\n\r\nSecond\u{001b}]52;c;payload\u{0007}!"}}),
    );
    assert!(text.contains("reader@example.com"));
    assert!(text.contains("First\n\nSecond!"));
    assert!(!text.contains("payload"));
    assert_eq!(safe_text("normal\u{202e}hidden\u{0008}"), "normalhidden");
}

#[test]
fn attachment_sizes_and_event_context_are_readable() {
    let page = Page {
        items: vec![json!({"name":"report.pdf", "size":1536})],
        next_cursor: None,
        truncated: false,
    };
    assert!(page_text(&page, PageKind::Attachments).contains("1.5 KiB"));
    let page = Page {
        items: vec![
            json!({"subject":"Planning", "start":{"dateTime":"2026-09-08T09:00:00", "timeZone":"Europe/Amsterdam"}, "location":{"displayName":"Room 2"}}),
        ],
        next_cursor: None,
        truncated: false,
    };
    let text = page_text(&page, PageKind::Events);
    assert!(text.contains("Europe/Amsterdam"));
    assert!(text.contains("Room 2"));
}
