use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use serde_json::Value;

use crate::{graph::Page, presentation::safe_text};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Request {
    Messages {
        folder: String,
        query: String,
        cursor: Option<String>,
    },
    Message(String),
    Folders {
        parent: Option<String>,
        cursor: Option<String>,
    },
}

pub(super) enum Response {
    Page(Page),
    Message(Value),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Focus {
    Messages,
    Preview,
    Folders,
}

pub(super) enum Action {
    None,
    Quit,
    Fetch(Request),
}

pub(super) struct App {
    pub context: String,
    pub folder: String,
    pub folder_name: String,
    pub query: String,
    pub input: Option<String>,
    pub messages: Vec<Value>,
    pub selection: ListState,
    pub detail: Option<Value>,
    pub scroll: u16,
    pub max_scroll: u16,
    pub next_cursor: Option<String>,
    pub folders: Vec<Value>,
    pub folder_selection: ListState,
    pub folder_parent: Option<String>,
    pub folder_history: Vec<Option<String>>,
    pub folders_cursor: Option<String>,
    pub focus: Focus,
    pub pending: Option<Request>,
    pub retry: Option<Request>,
    pub error: Option<String>,
    pub status: String,
    pub help: bool,
    pub color: bool,
}

impl App {
    pub fn new(context: String, folder: String, color: bool) -> Self {
        Self {
            context,
            folder_name: folder.clone(),
            folder,
            query: String::new(),
            input: None,
            messages: vec![],
            selection: ListState::default(),
            detail: None,
            scroll: 0,
            max_scroll: 0,
            next_cursor: None,
            folders: vec![],
            folder_selection: ListState::default(),
            folder_parent: None,
            folder_history: vec![],
            folders_cursor: None,
            focus: Focus::Messages,
            pending: None,
            retry: None,
            error: None,
            status: "Connecting… · q to quit".into(),
            help: false,
            color,
        }
    }

    pub fn list_request(&self, more: bool) -> Request {
        Request::Messages {
            folder: self.folder.clone(),
            query: self.query.clone(),
            cursor: if more { self.next_cursor.clone() } else { None },
        }
    }

    pub fn folder_request(&self, more: bool) -> Request {
        Request::Folders {
            parent: self.folder_parent.clone(),
            cursor: if more {
                self.folders_cursor.clone()
            } else {
                None
            },
        }
    }

    pub fn selected(&self) -> Option<&Value> {
        self.selection.selected().and_then(|i| self.messages.get(i))
    }

    pub fn preview_request(&mut self) -> Action {
        self.detail = None;
        self.scroll = 0;
        self.max_scroll = 0;
        match self.selected().and_then(|v| v["id"].as_str()) {
            Some(id) => Action::Fetch(Request::Message(id.into())),
            None => Action::None,
        }
    }

    pub fn begin(&mut self, request: Request) {
        match &request {
            Request::Messages { cursor: None, .. } => {
                self.messages.clear();
                self.selection = ListState::default();
                self.detail = None;
                self.scroll = 0;
                self.next_cursor = None;
            }
            Request::Folders { cursor: None, .. } => {
                self.folders.clear();
                self.folder_selection = ListState::default();
                self.folders_cursor = None;
            }
            _ => {}
        }
        self.status = match &request {
            Request::Messages {
                cursor: Some(_), ..
            } => "Loading more messages…",
            Request::Messages { .. } => "Loading messages…",
            Request::Message(_) => "Loading preview…",
            Request::Folders { .. } => "Loading folders…",
        }
        .into();
        self.pending = Some(request);
        self.error = None;
        self.retry = None;
    }

    pub fn complete(&mut self, request: &Request, response: Result<Response, String>) -> Action {
        if self.pending.as_ref() != Some(request) {
            return Action::None;
        }
        self.pending = None;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.error = Some(safe_text(&error));
                self.retry = Some(request.clone());
                self.status = "Request failed · r retry · q quit".into();
                return Action::None;
            }
        };
        match (request, response) {
            (Request::Message(id), Response::Message(value)) => {
                if self.selected().and_then(|v| v["id"].as_str()) == Some(id.as_str()) {
                    self.detail = Some(value);
                }
                self.status = "Preview only · mailbox unchanged".into();
            }
            (Request::Messages { cursor, .. }, Response::Page(page)) => {
                let more = cursor.is_some();
                if !more {
                    self.messages.clear();
                    self.selection = ListState::default();
                    self.detail = None;
                    self.scroll = 0;
                }
                append_unique(&mut self.messages, page.items);
                self.next_cursor = page.next_cursor.filter(|_| page.truncated);
                if self.selection.selected().is_none() && !self.messages.is_empty() {
                    self.selection.select(Some(0));
                }
                self.status = if self.messages.is_empty() && self.next_cursor.is_some() {
                    "No matches in this page · n to continue searching".into()
                } else {
                    "Preview only · mailbox unchanged".into()
                };
                if !more || self.detail.is_none() {
                    return self.preview_request();
                }
            }
            (Request::Folders { cursor, .. }, Response::Page(page)) => {
                if cursor.is_none() {
                    self.folders.clear();
                    self.folder_selection = ListState::default();
                }
                append_unique(&mut self.folders, page.items);
                self.folders_cursor = page.next_cursor.filter(|_| page.truncated);
                if self.folder_selection.selected().is_none() && !self.folders.is_empty() {
                    self.folder_selection.select(Some(0));
                }
                self.status =
                    "Enter opens mail · → browses child folders · Backspace goes up".into();
            }
            _ => {}
        }
        Action::None
    }

    pub fn key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        if self.help {
            self.help = false;
            return Action::None;
        }
        if let Some(input) = &mut self.input {
            match key.code {
                KeyCode::Esc => self.input = None,
                KeyCode::Enter => {
                    self.query = input.trim().to_owned();
                    self.input = None;
                    self.messages.clear();
                    self.selection = ListState::default();
                    self.detail = None;
                    self.next_cursor = None;
                    self.focus = Focus::Messages;
                    return Action::Fetch(self.list_request(false));
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.clear()
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && !ch.is_control()
                        && input.len() < 2048 =>
                {
                    input.push(ch)
                }
                _ => {}
            }
            return Action::None;
        }
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => {
                self.help = true;
                return Action::None;
            }
            KeyCode::Char('r') => {
                return Action::Fetch(self.retry.clone().unwrap_or_else(|| {
                    if self.focus == Focus::Folders {
                        self.folder_request(false)
                    } else if self.focus == Focus::Preview {
                        self.selected()
                            .and_then(|v| v["id"].as_str())
                            .map(|id| Request::Message(id.into()))
                            .unwrap_or_else(|| self.list_request(false))
                    } else {
                        self.list_request(false)
                    }
                }));
            }
            _ => {}
        }
        if self.focus == Focus::Folders {
            match key.code {
                KeyCode::Esc | KeyCode::Char('f') => {
                    self.focus = Focus::Messages;
                    return Action::Fetch(self.list_request(false));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    move_selection(&mut self.folder_selection, self.folders.len(), 1)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    move_selection(&mut self.folder_selection, self.folders.len(), -1)
                }
                KeyCode::Enter | KeyCode::Right => {
                    if self.pending.is_some() {
                        return Action::None;
                    }
                    if let Some(folder) = self
                        .folder_selection
                        .selected()
                        .and_then(|i| self.folders.get(i))
                        && let Some(id) = folder["id"].as_str()
                    {
                        if key.code == KeyCode::Right {
                            self.folder_history.push(self.folder_parent.clone());
                            self.folder_parent = Some(id.into());
                            return Action::Fetch(self.folder_request(false));
                        }
                        self.folder = id.into();
                        self.folder_name = text(folder, "/displayName");
                        self.query.clear();
                        self.next_cursor = None;
                        self.messages.clear();
                        self.detail = None;
                        self.selection = ListState::default();
                        self.focus = Focus::Messages;
                        return Action::Fetch(self.list_request(false));
                    }
                }
                KeyCode::Backspace | KeyCode::Left => {
                    if let Some(parent) = self.folder_history.pop() {
                        self.folder_parent = parent;
                        return Action::Fetch(self.folder_request(false));
                    }
                }
                KeyCode::Char('n') if self.folders_cursor.is_some() && self.pending.is_none() => {
                    return Action::Fetch(self.folder_request(true));
                }
                _ => {}
            }
            return Action::None;
        }
        match key.code {
            KeyCode::Char('/') => self.input = Some(self.query.clone()),
            KeyCode::Char('f') => {
                self.focus = Focus::Folders;
                return Action::Fetch(self.folder_request(false));
            }
            KeyCode::Tab | KeyCode::Enter => {
                self.focus = if self.focus == Focus::Preview {
                    Focus::Messages
                } else {
                    Focus::Preview
                };
            }
            KeyCode::Esc => {
                if self.focus == Focus::Preview {
                    self.focus = Focus::Messages;
                } else if !self.query.is_empty() {
                    self.query.clear();
                    return Action::Fetch(self.list_request(false));
                } else {
                    return Action::Quit;
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                let down = matches!(key.code, KeyCode::Down | KeyCode::Char('j'));
                if self.focus == Focus::Preview {
                    self.scroll = if down {
                        self.scroll.saturating_add(1).min(self.max_scroll)
                    } else {
                        self.scroll.saturating_sub(1)
                    };
                } else if !matches!(self.pending, Some(Request::Messages { .. })) {
                    let old = self.selection.selected();
                    move_selection(
                        &mut self.selection,
                        self.messages.len(),
                        if down { 1 } else { -1 },
                    );
                    if old != self.selection.selected() {
                        return self.preview_request();
                    }
                }
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10).min(self.max_scroll),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Home if self.focus == Focus::Preview => self.scroll = 0,
            KeyCode::End if self.focus == Focus::Preview => self.scroll = self.max_scroll,
            KeyCode::Char('n') if self.next_cursor.is_some() && self.pending.is_none() => {
                return Action::Fetch(self.list_request(true));
            }
            _ => {}
        }
        Action::None
    }
}

fn move_selection(state: &mut ListState, length: usize, delta: isize) {
    if length == 0 {
        state.select(None);
        return;
    }
    state.select(Some(
        state
            .selected()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(length - 1),
    ));
}

fn append_unique(items: &mut Vec<Value>, new: Vec<Value>) {
    let mut ids = items
        .iter()
        .filter_map(|v| v["id"].as_str().map(str::to_owned))
        .collect::<std::collections::HashSet<_>>();
    items.extend(
        new.into_iter()
            .filter(|v| v["id"].as_str().is_none_or(|id| ids.insert(id.to_owned()))),
    );
}

pub(super) fn text(value: &Value, path: &str) -> String {
    safe_text(value.pointer(path).and_then(Value::as_str).unwrap_or(""))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
