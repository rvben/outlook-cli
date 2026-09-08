use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap},
};

use super::model::{App, Focus, Request, text};
use crate::presentation::safe_text;

fn accent(app: &App) -> Style {
    let style = Style::default().add_modifier(Modifier::BOLD);
    if app.color {
        style.fg(Color::Cyan)
    } else {
        style
    }
}

fn border(app: &App, active: bool) -> Style {
    if active {
        accent(app)
    } else {
        Style::default()
    }
}

fn panel<'a>(app: &App, title: String, active: bool) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border(app, active))
        .padding(Padding::horizontal(1))
}

pub(super) fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 36 || area.height < 12 {
        frame.render_widget(
            Paragraph::new("Outlook\nEnlarge to at least 36 × 12.\nq to quit")
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(2),
        Constraint::Length(2),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Outlook ", accent(app)),
            Span::raw(format!("  {}", safe_text(&app.context))),
        ])),
        rows[0],
    );
    if app.focus == Focus::Folders {
        folders(frame, rows[1], app);
    } else if area.width >= 100 {
        let columns = Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(rows[1]);
        messages(frame, columns[0], app);
        preview(frame, columns[1], app);
    } else if app.focus == Focus::Preview {
        preview(frame, rows[1], app);
    } else {
        messages(frame, rows[1], app);
    }

    if let Some(input) = &app.input {
        let line = Line::raw(format!("Search: {}▏", safe_text(input)));
        let offset = line
            .width()
            .saturating_sub(rows[2].width as usize)
            .min(u16::MAX as usize) as u16;
        let first = Rect::new(rows[2].x, rows[2].y, rows[2].width, 1);
        let second = Rect::new(rows[2].x, rows[2].y + 1, rows[2].width, 1);
        frame.render_widget(
            Paragraph::new(line).scroll((0, offset)).style(accent(app)),
            first,
        );
        frame.render_widget(
            Paragraph::new("Enter apply · Esc cancel · ^U clear"),
            second,
        );
    } else if let Some(error) = &app.error {
        frame.render_widget(
            Paragraph::new(format!("{error}\nPress r to retry; q to quit."))
                .wrap(Wrap { trim: false }),
            rows[2],
        );
    } else {
        let count = if app.focus == Focus::Folders {
            app.folders.len()
        } else {
            app.messages.len()
        };
        let more = if app.focus == Focus::Folders {
            app.folders_cursor.is_some()
        } else {
            app.next_cursor.is_some()
        };
        frame.render_widget(
            Paragraph::new(format!(
                "{}\n{count} loaded{}",
                app.status,
                if more { " · n load more" } else { "" }
            )),
            rows[2],
        );
    }
    let help = if app.focus == Focus::Folders {
        "j/k move · Enter open · → child\n← up · n more · Esc back · q quit"
    } else if area.width < 65 && app.focus == Focus::Preview {
        "j/k scroll · Tab back · / search\nPgUp/PgDn · ? help · q quit"
    } else if area.width < 65 {
        "j/k move · Enter read · / search\nf folders  r retry  ? help  q quit"
    } else {
        "j/k move · Tab list/preview · / search · f folders\nn more · r refresh/retry · ? help · q quit"
    };
    frame.render_widget(Paragraph::new(help), rows[3]);
    if app.help {
        help_overlay(frame, area, app);
    }
}

fn messages(frame: &mut Frame, area: Rect, app: &mut App) {
    let scope = if app.query.is_empty() {
        app.folder_name.clone()
    } else {
        format!("{} / {}", app.folder_name, app.query)
    };
    let block = panel(
        app,
        format!(" {} ", safe_text(&scope)),
        app.focus == Focus::Messages,
    );
    if app.messages.is_empty() {
        let empty = if matches!(app.pending, Some(Request::Messages { .. })) {
            "Loading messages…\n\nYou can quit with q."
        } else if app.error.is_some() {
            "Couldn't load messages.\n\nPress r to retry.\nPress f to choose another folder."
        } else if app.next_cursor.is_some() {
            "No matches in this page.\n\nPress n to continue searching."
        } else if !app.query.is_empty() {
            "No matching messages.\n\nPress / to change your search.\nPress Esc to clear it."
        } else {
            "This folder has no messages.\n\nPress f to choose another folder.\nPress r to refresh."
        };
        let empty = if let Some(error) = &app.error {
            format!("{empty}\n\n{error}")
        } else {
            empty.into()
        };
        frame.render_widget(
            Paragraph::new(empty)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let items = app
        .messages
        .iter()
        .map(|value| {
            let subject = text(value, "/subject");
            let mut from = text(value, "/from/emailAddress/name");
            if from.is_empty() {
                from = text(value, "/from/emailAddress/address");
            }
            if from.is_empty() {
                from = "Unknown sender".into();
            }
            let date = text(value, "/receivedDateTime");
            let date = date.get(..16).unwrap_or(&date).replace('T', " ");
            let unread = value["isRead"] == false;
            ListItem::new(vec![
                Line::styled(
                    format!(
                        "{} {}",
                        if unread { "●" } else { " " },
                        if subject.is_empty() {
                            "(no subject)"
                        } else {
                            &subject
                        }
                    ),
                    if unread {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
                Line::raw(format!("  {from}")),
                Line::raw(format!(
                    "  {date}{}",
                    if value["hasAttachments"] == true {
                        " · attachment"
                    } else {
                        ""
                    }
                )),
                Line::raw(""),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_symbol("› ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        &mut app.selection,
    );
}

fn preview(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = panel(
        app,
        " Message · PgUp/PgDn scroll ".into(),
        app.focus == Focus::Preview,
    );
    let inner = block.inner(area);
    let Some(value) = app.detail.as_ref().or_else(|| app.selected()) else {
        frame.render_widget(
            Paragraph::new("Select a message to read it here.")
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    };
    let subject = text(value, "/subject");
    let mut lines = vec![
        Line::styled(
            if subject.is_empty() {
                "(no subject)".into()
            } else {
                subject
            },
            accent(app),
        ),
        Line::raw(""),
    ];
    let name = text(value, "/from/emailAddress/name");
    let address = text(value, "/from/emailAddress/address");
    lines.push(Line::raw(format!(
        "From  {name}{}",
        if address.is_empty() {
            String::new()
        } else {
            format!(" <{address}>")
        }
    )));
    for (key, label) in [("toRecipients", "To"), ("ccRecipients", "Cc")] {
        if let Some(recipients) = value[key].as_array() {
            let names = recipients
                .iter()
                .map(|v| text(v, "/emailAddress/address"))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            if !names.is_empty() {
                lines.push(Line::raw(format!("{label:<6}{names}")));
            }
        }
    }
    let date = text(value, "/receivedDateTime");
    if !date.is_empty() {
        lines.push(Line::raw(format!("Date  {date}")));
    }
    if value["hasAttachments"] == true {
        lines.push(Line::raw("Attachments included"));
    }
    lines.push(Line::raw(""));
    if app.detail.is_none() {
        lines.push(Line::styled(
            if app.error.is_some() {
                "Preview unavailable · r to retry"
            } else {
                "Loading full message…"
            },
            accent(app),
        ));
        lines.push(Line::raw(""));
        if let Some(error) = &app.error {
            lines.push(Line::raw(error.clone()));
            lines.push(Line::raw(""));
        }
    }
    let body = value
        .pointer("/body/content")
        .and_then(|v| v.as_str())
        .or_else(|| value["bodyPreview"].as_str())
        .unwrap_or("");
    if body.is_empty() && app.detail.is_some() {
        lines.push(Line::raw("(empty message)"));
    } else {
        lines.extend(
            safe_text(&body.replace("\r\n", "\n"))
                .lines()
                .map(|line| Line::raw(line.to_owned())),
        );
    }
    let id = text(value, "/id");
    if !id.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::raw(format!("Message ID  {id}")));
    }
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    app.max_scroll = paragraph
        .line_count(inner.width)
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize) as u16;
    app.scroll = app.scroll.min(app.max_scroll);
    frame.render_widget(paragraph.scroll((app.scroll, 0)).block(block), area);
}

fn folders(frame: &mut Frame, area: Rect, app: &mut App) {
    let title = format!(
        " Folders · {} ",
        if app.folder_parent.is_some() {
            "child folders"
        } else {
            "mailbox root"
        }
    );
    let block = panel(app, title, true);
    if app.folders.is_empty() {
        let message = if app.pending.is_some() {
            "Loading folders…"
        } else if app.error.is_some() {
            "Couldn't load folders. Press r to retry."
        } else {
            "No folders here.\nPress Backspace for the parent, or Esc to return to mail."
        };
        let message = if let Some(error) = &app.error {
            format!("{message}\n\n{error}")
        } else {
            message.into()
        };
        frame.render_widget(
            Paragraph::new(message)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let items = app
        .folders
        .iter()
        .map(|v| {
            let mut info = Vec::new();
            if let Some(n) = v["unreadItemCount"].as_u64() {
                info.push(format!("{n} unread"));
            }
            if let Some(n) = v["childFolderCount"].as_u64() {
                info.push(format!("{n} child folders"));
            }
            ListItem::new(vec![
                Line::raw(text(v, "/displayName")),
                Line::raw(info.join(" · ")),
                Line::raw(""),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_symbol("› ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        &mut app.folder_selection,
    );
}

fn help_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width.min(76);
    let height = area.height.min(22);
    let rect = Rect::new(
        (area.width - width) / 2,
        (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let guide = if width < 64 || height < 20 {
        "j/k Move or scroll preview\nEnter/Tab Switch pane\n/ Search · Esc Cancel/back\nf Folders · ←/→ Up/children\nn More · r Retry/refresh\nPgUp/PgDn Scroll preview\nq or Ctrl+c Quit\n\nAny key closes this guide."
    } else {
        "Move             ↑/↓ or j/k\nRead / back      Enter or Tab\nScroll message   ↑/↓, PgUp/PgDn, Home/End\nSearch folder    /, type query, Enter\nClear search     Esc from message list\nChoose folder    f, move, Enter\nChild folders    → ; ← or Backspace goes up\nLoad next page   n\nRefresh / retry  r\nQuit             q or Ctrl+c\n\nGraph search uses Outlook syntax.\nDesktop search matches literal subject/sender text.\nPreviewing does not mark messages as read.\n\nPress any key to close this guide."
    };
    frame.render_widget(
        Paragraph::new(guide)
            .block(panel(app, " Keyboard guide ".into(), true))
            .wrap(Wrap { trim: false }),
        rect,
    );
}
