use super::model::text;
use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn key(app: &mut App, code: KeyCode) -> Action {
    app.key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn page(items: Vec<Value>, cursor: Option<&str>) -> Response {
    Response::Page(Page {
        items,
        next_cursor: cursor.map(str::to_owned),
        truncated: cursor.is_some(),
    })
}

#[test]
fn selection_discards_old_preview_and_ignores_stale_responses() {
    let mut app = demo_app();
    let first = Request::Message("demo-message-0".into());
    let Action::Fetch(second) = key(&mut app, KeyCode::Down) else {
        panic!("selection should load a preview")
    };
    assert!(app.detail.is_none());
    assert_eq!(app.scroll, 0);
    app.begin(second.clone());
    app.complete(&first, Ok(demo_response(&first)));
    assert!(app.detail.is_none());
    assert_eq!(app.pending, Some(second.clone()));
    app.complete(&second, Ok(demo_response(&second)));
    assert_eq!(app.detail.unwrap()["id"], "demo-message-1");
}

#[test]
fn empty_search_pages_preserve_cursor_and_retry_same_scope() {
    let mut app = demo_app();
    app.query = "quarterly report".into();
    let request = app.list_request(false);
    app.begin(request.clone());
    app.complete(&request, Ok(page(vec![], Some("cursor-1"))));
    assert!(app.messages.is_empty());
    assert!(app.status.contains("n to continue"));
    let Action::Fetch(next) = key(&mut app, KeyCode::Char('n')) else {
        panic!()
    };
    assert_eq!(
        next,
        Request::Messages {
            folder: "inbox".into(),
            query: "quarterly report".into(),
            cursor: Some("cursor-1".into())
        }
    );
    app.begin(next.clone());
    app.complete(&next, Err("Connection lost".into()));
    let Action::Fetch(retry) = key(&mut app, KeyCode::Char('r')) else {
        panic!()
    };
    assert_eq!(retry, next);
    app.begin(retry.clone());
    app.complete(
        &retry,
        Ok(page(
            vec![json!({"id":"found","subject":"Quarterly report"})],
            None,
        )),
    );
    assert_eq!(app.selected().unwrap()["id"], "found");
}

#[test]
fn next_page_deduplicates_without_moving_selection() {
    let mut app = demo_app();
    let original = app.messages.len();
    app.next_cursor = Some("next".into());
    let request = app.list_request(true);
    app.begin(request.clone());
    app.complete(
        &request,
        Ok(page(
            vec![app.messages[0].clone(), json!({"id":"new"})],
            None,
        )),
    );
    assert_eq!(app.messages.len(), original + 1);
    assert_eq!(app.selection.selected(), Some(0));
    assert!(app.detail.is_some());
}

#[test]
fn search_can_be_cancelled_and_letters_do_not_trigger_shortcuts() {
    let mut app = demo_app();
    key(&mut app, KeyCode::Char('/'));
    for ch in "query".chars() {
        assert!(matches!(key(&mut app, KeyCode::Char(ch)), Action::None));
    }
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.query, "");
    key(&mut app, KeyCode::Char('/'));
    key(&mut app, KeyCode::Char('東'));
    let Action::Fetch(request) = key(&mut app, KeyCode::Enter) else {
        panic!()
    };
    assert_eq!(app.query, "東");
    assert!(app.messages.is_empty());
    assert_eq!(request, app.list_request(false));
}

#[test]
fn folder_navigation_tracks_parents_and_clears_search_when_opening() {
    let mut app = demo_app();
    app.query = "old filter".into();
    let Action::Fetch(request) = key(&mut app, KeyCode::Char('f')) else {
        panic!()
    };
    app.begin(request.clone());
    app.complete(&request, Ok(demo_response(&request)));
    let Action::Fetch(children) = key(&mut app, KeyCode::Right) else {
        panic!()
    };
    assert_eq!(
        children,
        Request::Folders {
            parent: Some("inbox".into()),
            cursor: None
        }
    );
    app.begin(children.clone());
    app.complete(&children, Ok(page(vec![], None)));
    let Action::Fetch(parent) = key(&mut app, KeyCode::Backspace) else {
        panic!()
    };
    assert_eq!(
        parent,
        Request::Folders {
            parent: None,
            cursor: None
        }
    );
    app.begin(parent.clone());
    app.complete(&parent, Ok(demo_response(&parent)));
    key(&mut app, KeyCode::Down);
    let Action::Fetch(open) = key(&mut app, KeyCode::Enter) else {
        panic!()
    };
    assert_eq!(
        open,
        Request::Messages {
            folder: "sentitems".into(),
            query: String::new(),
            cursor: None
        }
    );
    assert!(app.messages.is_empty());
    assert_eq!(app.folder_name, "Sent items");
}

#[test]
fn preview_scroll_is_bounded_and_navigation_remains_available_during_requests() {
    let mut app = demo_app();
    app.max_scroll = 12;
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::PageDown);
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.scroll, 12);
    key(&mut app, KeyCode::Home);
    assert_eq!(app.scroll, 0);
    app.begin(app.list_request(false));
    assert!(matches!(key(&mut app, KeyCode::Char('q')), Action::Quit));
}

fn frame(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view::draw(f, app)).unwrap();
    let buffer = terminal.backend().buffer();
    if let Some(directory) = std::env::var_os("OUTLOOK_TUI_CAPTURE_DIR") {
        static SEQUENCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let cells = buffer.content().iter().map(|cell| json!({
            "symbol":cell.symbol(), "fg":format!("{:?}",cell.fg), "bg":format!("{:?}",cell.bg),
            "bold":cell.modifier.contains(ratatui::style::Modifier::BOLD),
            "reversed":cell.modifier.contains(ratatui::style::Modifier::REVERSED),
        })).collect::<Vec<_>>();
        std::fs::write(
            directory.join(format!("frame-{sequence}-{width}x{height}.json")),
            serde_json::to_vec(&json!({"width":width,"height":height,"cells":cells})).unwrap(),
        )
        .unwrap();
    }
    let result = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !app.color {
        for cell in buffer.content() {
            assert_eq!(cell.fg, Color::Reset);
            assert_eq!(cell.bg, Color::Reset);
        }
    }
    result
}

#[test]
fn responsive_views_keep_navigation_and_content_visible() {
    let mut app = demo_app();
    let wide = frame(&mut app, 120, 32);
    assert!(wide.contains("Design review"));
    assert!(wide.contains("Morgan"));
    assert!(wide.contains("revised layouts"));
    let narrow = frame(&mut app, 40, 20);
    assert!(narrow.contains("Design review"));
    assert!(narrow.contains("Enter read"));
    key(&mut app, KeyCode::Enter);
    let preview = frame(&mut app, 40, 20);
    assert!(preview.contains("revised layouts"));
    key(&mut app, KeyCode::Esc);
    assert!(frame(&mut app, 40, 20).contains("Design review"));
    assert!(frame(&mut app, 20, 8).contains("Enlarge"));
}

#[test]
fn views_handle_empty_error_loading_and_untrusted_content() {
    let mut app = demo_app();
    app.messages.clear();
    app.detail = None;
    assert!(frame(&mut app, 80, 24).contains("no messages"));
    let request = app.list_request(false);
    app.begin(request.clone());
    assert!(frame(&mut app, 80, 24).contains("Loading messages"));
    app.complete(
        &request,
        Err("Session expired: run outlook auth login".into()),
    );
    assert!(frame(&mut app, 80, 24).contains("Session expired"));
    let malicious =
        json!({"subject":"Title\u{001b}[2J", "from":{"emailAddress":{"name":"name\nspoof"}}});
    assert_eq!(text(&malicious, "/subject"), "Title");
    assert_eq!(text(&malicious, "/from/emailAddress/name"), "name spoof");
}

#[tokio::test]
async fn live_source_reads_graph_mail_search_folders_and_preview_without_writes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1.0/me/mailFolders/inbox/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"value":[{"id":"one","subject":"Test"}]})),
        )
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1.0/me/messages/one"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":"one","body":{"content":"Full message"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1.0/me/mailFolders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":[]})))
        .expect(1)
        .mount(&server)
        .await;
    let source = Source::Mail(MailBackend::Graph(
        crate::graph::GraphClient::with_base("test".into(), &format!("{}/v1.0", server.uri()))
            .unwrap(),
    ));
    for request in [
        Request::Messages {
            folder: "inbox".into(),
            query: String::new(),
            cursor: None,
        },
        Request::Messages {
            folder: "inbox".into(),
            query: "Test".into(),
            cursor: None,
        },
        Request::Message("one".into()),
        Request::Folders {
            parent: None,
            cursor: None,
        },
    ] {
        source.fetch(&request).await.unwrap();
    }
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|request| request.method == "GET"));
    assert!(requests.iter().any(|r| {
        r.url
            .query_pairs()
            .any(|(key, value)| key == "$search" && value.contains("Test"))
    }));
}

#[tokio::test]
async fn dropping_a_job_cancels_the_read_instead_of_detaching_it() {
    let mut app = demo_app();
    let job = start(
        &mut app,
        Arc::new(Source::Demo),
        Request::Message("demo-message-0".into()),
    );
    let abort = job.0.abort_handle();
    drop(job);
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(abort.is_finished());
    assert!(matches!(key(&mut app, KeyCode::Char('q')), Action::Quit));
}

#[test]
fn views_keep_focus_help_and_long_search_input_visible() {
    let mut app = demo_app();
    app.color = true;
    let wide = frame(&mut app, 120, 32);
    assert!(wide.contains("Outlook"));
    let Action::Fetch(request) = key(&mut app, KeyCode::Char('f')) else {
        panic!()
    };
    app.begin(request.clone());
    app.complete(&request, Ok(demo_response(&request)));
    assert!(frame(&mut app, 80, 24).contains("Sent items"));
    app.help = true;
    assert!(frame(&mut app, 36, 12).contains("Any key closes"));
    // Start a fresh inbox so the capture represents a completed live transition.
    app = demo_app();
    app.color = true;
    app.input = Some("A very long search query that needs horizontal scrolling END".into());
    assert!(frame(&mut app, 36, 12).contains("END▏"));
    app.input = None;
    app.focus = super::model::Focus::Preview;
    assert!(frame(&mut app, 36, 12).contains("Tab back"));
}
