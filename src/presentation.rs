//! Human-readable views shared by Graph and classic Outlook.
use serde_json::Value;

use crate::{graph::Page, output::accent};

#[derive(Clone, Copy)]
pub enum PageKind {
    Messages,
    Folders,
    Events,
    Attachments,
}

/// Mail content is untrusted: never pass terminal controls through to the screen.
/// Preserve body line breaks and tabs, but remove escape sequences and bidi controls.
pub fn safe_text(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.next() {
                Some('[') => {
                    for ch in chars.by_ref() {
                        if ('@'..='~').contains(&ch) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(ch) = chars.next() {
                        if ch == '\x07' || (ch == '\x1b' && chars.next() == Some('\\')) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if (!ch.is_control() || ch == '\n' || ch == '\t')
            && !matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            result.push(ch);
        }
    }
    result
}

fn field(value: &Value, path: &str) -> String {
    safe_text(value.pointer(path).and_then(Value::as_str).unwrap_or(""))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn title(value: &Value, path: &str, fallback: &str) -> String {
    let text = field(value, path);
    if text.is_empty() {
        fallback.into()
    } else {
        text
    }
}

fn person(value: &Value) -> String {
    let name = field(value, "/emailAddress/name");
    let address = field(value, "/emailAddress/address");
    match (name.is_empty(), address.is_empty() || address == name) {
        (true, _) => address,
        (false, true) => name,
        (false, false) => format!("{name} <{address}>"),
    }
}

fn recipients(value: &Value, key: &str) -> String {
    value[key]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(person)
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn metadata(lines: &mut Vec<String>, label: &str, value: String) {
    if !value.is_empty() {
        lines.push(format!("  {label:<8} {value}"));
    }
}

fn item_text(value: &Value, kind: PageKind) -> String {
    let mut lines = Vec::new();
    match kind {
        PageKind::Messages => {
            let marker = if value["isRead"] == false {
                "● "
            } else {
                "  "
            };
            lines.push(format!(
                "{marker}{}",
                accent(&title(value, "/subject", "(no subject)"))
            ));
            metadata(&mut lines, "From", person(&value["from"]));
            metadata(&mut lines, "Received", field(value, "/receivedDateTime"));
            let mut flags = Vec::new();
            if value["isRead"] == false {
                flags.push("unread");
            }
            if value["hasAttachments"] == true {
                flags.push("attachments");
            }
            if value["importance"] == "high" {
                flags.push("high importance");
            }
            metadata(&mut lines, "Status", flags.join(" · "));
        }
        PageKind::Folders => {
            lines.push(format!(
                "  {}",
                accent(&title(value, "/displayName", "(unnamed folder)"))
            ));
            let mut counts = Vec::new();
            if let Some(count) = value["totalItemCount"].as_u64() {
                counts.push(format!("{count} total"));
            }
            if let Some(count) = value["unreadItemCount"].as_u64() {
                counts.push(format!("{count} unread"));
            }
            metadata(&mut lines, "Messages", counts.join(" · "));
        }
        PageKind::Events => {
            lines.push(format!(
                "  {}",
                accent(&title(value, "/subject", "(no subject)"))
            ));
            metadata(&mut lines, "Start", field(value, "/start/dateTime"));
            metadata(&mut lines, "End", field(value, "/end/dateTime"));
            metadata(&mut lines, "Timezone", field(value, "/start/timeZone"));
            metadata(
                &mut lines,
                "Location",
                field(value, "/location/displayName"),
            );
            if value["isAllDay"] == true {
                metadata(&mut lines, "Status", "all day".into());
            }
            if value["isCancelled"] == true {
                metadata(&mut lines, "Status", "cancelled".into());
            }
        }
        PageKind::Attachments => {
            lines.push(format!(
                "  {}",
                accent(&title(value, "/name", "(unnamed attachment)"))
            ));
            if let Some(bytes) = value["size"].as_u64() {
                metadata(&mut lines, "Size", file_size(bytes));
            }
            metadata(&mut lines, "Type", field(value, "/contentType"));
            if value["isInline"] == true {
                metadata(&mut lines, "Status", "inline".into());
            }
        }
    }
    metadata(&mut lines, "ID", field(value, "/id"));
    lines.join("\n")
}

fn file_size(bytes: u64) -> String {
    for (unit, divisor) in [("GiB", 1_u64 << 30), ("MiB", 1 << 20), ("KiB", 1 << 10)] {
        if bytes >= divisor {
            return format!("{:.1} {unit}", bytes as f64 / divisor as f64);
        }
    }
    format!("{bytes} B")
}

pub fn page_text(page: &Page, kind: PageKind) -> String {
    let (heading, empty) = match kind {
        PageKind::Messages => ("Messages", "No messages in this page."),
        PageKind::Folders => ("Folders", "No folders in this page."),
        PageKind::Events => ("Agenda", "No events in this page."),
        PageKind::Attachments => ("Attachments", "No attachments in this page."),
    };
    let mut sections = vec![accent(&format!("{heading} · {} shown", page.items.len()))];
    if page.items.is_empty() {
        sections.push(empty.into());
    }
    sections.extend(page.items.iter().map(|item| item_text(item, kind)));
    if page.truncated {
        sections.push(
            "More results are available. Repeat this command with the same filters and:".into(),
        );
        if let Some(cursor) = &page.next_cursor {
            // POSIX quoting also works in PowerShell for ordinary opaque tokens.
            sections.push(format!(
                "  --cursor '{}'",
                safe_text(cursor).replace('\'', "'\\''")
            ));
        } else {
            sections.push("Use --output json to inspect pagination details.".into());
        }
    }
    sections.join("\n\n")
}

pub fn message_text(value: &Value) -> String {
    let mut lines = vec![
        accent(&title(value, "/subject", "(no subject)")),
        String::new(),
    ];
    metadata(&mut lines, "From", person(&value["from"]));
    metadata(&mut lines, "To", recipients(value, "toRecipients"));
    metadata(&mut lines, "Cc", recipients(value, "ccRecipients"));
    metadata(&mut lines, "Date", field(value, "/receivedDateTime"));
    metadata(&mut lines, "ID", field(value, "/id"));
    let body = value
        .pointer("/body/content")
        .and_then(Value::as_str)
        .or_else(|| value["bodyPreview"].as_str())
        .unwrap_or("");
    lines.push(String::new());
    lines.push(if body.trim().is_empty() {
        "(empty message)".into()
    } else {
        safe_text(&body.replace("\r\n", "\n"))
    });
    lines.join("\n")
}
