//! Read-only keyboard inbox. Remote work is cancellable and never blocks input.
mod model;
#[cfg(test)]
mod tests;
mod view;

use crossterm::{
    cursor::Show,
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::{CrosstermBackend, TestBackend},
};
use serde_json::{Value, json};
use std::{
    io::{self, IsTerminal},
    sync::Arc,
    time::Duration,
};
use tokio::task::JoinHandle;

use crate::{backend::MailBackend, config, error::AppError, graph::Page};
use model::{Action, App, Request, Response};

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<Self, AppError> {
        enable_raw_mode()?;
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    }
}

// Dropping JoinHandle alone detaches work. Explicitly abort on quit and navigation.
struct Job(JoinHandle<Result<Response, String>>);
impl Drop for Job {
    fn drop(&mut self) {
        self.0.abort();
    }
}

enum Source {
    Mail(MailBackend),
    Demo,
}
impl Source {
    async fn fetch(&self, request: &Request) -> Result<Response, AppError> {
        if matches!(self, Self::Demo) {
            return Ok(demo_response(request));
        }
        let Self::Mail(mail) = self else {
            unreachable!()
        };
        match request {
            Request::Messages {
                folder,
                query,
                cursor,
            } => {
                let page = if query.is_empty() {
                    mail.messages(folder, 25, cursor.as_deref()).await?
                } else {
                    mail.search_messages(query, Some(folder), 25, cursor.as_deref())
                        .await?
                };
                Ok(Response::Page(page))
            }
            Request::Message(id) => mail.message(id).await.map(Response::Message),
            Request::Folders { parent, cursor } => mail
                .folders(parent.as_deref(), 50, cursor.as_deref())
                .await
                .map(Response::Page),
        }
    }
}

pub async fn run(
    profile: Option<&str>,
    folder: String,
    demo: bool,
    no_color: bool,
) -> Result<(), AppError> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(AppError::NonInteractive("`outlook tui` needs a terminal; use `outlook inbox --output json` for scripts, or `outlook tui --demo --snapshot` to inspect a sample screen".into()));
    }
    if folder.trim().is_empty() {
        return Err(AppError::InvalidInput("folder cannot be empty".into()));
    }
    // Connect before raw mode: credential errors remain readable, and no login is initiated.
    let (source, context) = if demo {
        (Source::Demo, "DEMO · sample mail".to_owned())
    } else {
        let (name, selected) = config::load(profile)?;
        (
            Source::Mail(MailBackend::connect(profile).await?),
            format!("{name} · {} · read-only browser", selected.backend.as_str()),
        )
    };
    let source = Arc::new(source);
    let color = !no_color
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").as_deref() != Ok("dumb");
    let mut app = App::new(context, folder, color);
    let _guard = TerminalGuard::enter()?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
        previous_hook(info);
    }));
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let request = app.list_request(false);
    let mut job = Some(start(&mut app, source.clone(), request));
    loop {
        if job.as_ref().is_some_and(|job| job.0.is_finished()) {
            let request = app.pending.clone();
            let result = (&mut job.as_mut().unwrap().0)
                .await
                .unwrap_or_else(|error| Err(format!("Mailbox request stopped: {error}")));
            job = None;
            if let Some(request) = request
                && let Action::Fetch(next) = app.complete(&request, result)
            {
                job = Some(start(&mut app, source.clone(), next));
            }
        }
        terminal.draw(|frame| view::draw(frame, &mut app))?;
        if event::poll(Duration::from_millis(40))?
            && let Event::Key(key) = event::read()?
            && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
        {
            match app.key(key) {
                Action::Quit => break,
                Action::Fetch(request) => {
                    // Abort before starting the replacement, especially for the COM bridge.
                    drop(job.take());
                    job = Some(start(&mut app, source.clone(), request));
                }
                Action::None => {}
            }
        }
        tokio::task::yield_now().await;
    }
    Ok(())
}

fn start(app: &mut App, source: Arc<Source>, request: Request) -> Job {
    app.begin(request.clone());
    Job(tokio::spawn(async move {
        if matches!(request, Request::Message(_)) {
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
        tokio::time::timeout(Duration::from_secs(50), source.fetch(&request))
            .await
            .map_err(|_| "Mailbox request timed out. Press r to retry.".to_owned())?
            .map_err(|error| error.to_string())
    }))
}

/// Deterministic offline frame, using the same widgets as the live inbox.
pub fn snapshot(width: u16, height: u16) -> Result<String, AppError> {
    if !(36..=240).contains(&width) || !(12..=100).contains(&height) {
        return Err(AppError::InvalidInput(
            "sample screen must be 36–240 columns by 12–100 rows".into(),
        ));
    }
    let mut app = demo_app();
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("test backend is infallible");
    terminal
        .draw(|frame| view::draw(frame, &mut app))
        .expect("test backend is infallible");
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_owned());
    }
    Ok(lines.join("\n"))
}

fn demo_app() -> App {
    let mut app = App::new("DEMO · sample mail".into(), "inbox".into(), false);
    let request = app.list_request(false);
    app.begin(request.clone());
    if let Action::Fetch(next) = app.complete(&request, Ok(demo_response(&request))) {
        app.begin(next.clone());
        app.complete(&next, Ok(demo_response(&next)));
    }
    app
}

fn demo_messages() -> Vec<Value> {
    [
        ("Design review · Thursday", "Morgan", "The revised layouts are ready for review.\n\nI focused on three things:\n\n• Keeping the message list easy to scan\n• Giving long messages enough room to breathe\n• Making every action work from the keyboard\n\nCan we walk through the narrow-terminal layout on Thursday?\n\nThanks,\nMorgan", false, true),
        ("Your weekly project digest", "Project updates", "This week: the release checks passed and the documentation is ready.\n\nNext week: review the onboarding flow and collect feedback.", true, false),
        ("Lunch plans?", "Casey", "Would noon work for lunch?\n\nThere is a new place near the office we could try.", false, false),
        ("Release checklist — final notes", "Taylor", "Everything is ready for the release review.\n\nPlease check the notes before our meeting.", true, true),
    ].iter().enumerate().map(|(i, (subject, sender, body, read, attachments))| json!({
        "id":format!("demo-message-{i}"), "subject":subject,
        "from":{"emailAddress":{"name":sender,"address":format!("sender{i}@example.com")}},
        "toRecipients":[{"emailAddress":{"address":"reader@example.com"}}],
        "receivedDateTime":format!("2026-09-08T{:02}:30:00Z", 10-i),
        "isRead":read,"hasAttachments":attachments,"bodyPreview":body,
        "body":{"contentType":"text","content":body}
    })).collect()
}

fn demo_response(request: &Request) -> Response {
    match request {
        Request::Message(id) => Response::Message(
            demo_messages()
                .into_iter()
                .find(|v| v["id"] == *id)
                .unwrap_or(Value::Null),
        ),
        Request::Messages { folder, query, .. } => {
            let items = if folder == "inbox" {
                demo_messages()
                    .into_iter()
                    .filter(|v| {
                        format!("{} {}", v["subject"], v["from"])
                            .to_lowercase()
                            .contains(&query.to_lowercase())
                    })
                    .collect()
            } else {
                vec![]
            };
            Response::Page(Page {
                items,
                next_cursor: None,
                truncated: false,
            })
        }
        Request::Folders { parent, .. } => Response::Page(Page {
            items: if parent.is_none() {
                vec![
                    json!({"id":"inbox", "displayName":"Inbox", "unreadItemCount":2, "childFolderCount":0}),
                    json!({"id":"sentitems", "displayName":"Sent items", "unreadItemCount":0, "childFolderCount":0}),
                    json!({"id":"drafts", "displayName":"Drafts", "unreadItemCount":0, "childFolderCount":0}),
                ]
            } else {
                vec![]
            },
            next_cursor: None,
            truncated: false,
        }),
    }
}
