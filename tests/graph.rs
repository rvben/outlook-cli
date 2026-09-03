use outlook_cli::graph::GraphClient;
use serde_json::json;
use wiremock::matchers::{body_bytes, body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn inbox_requests_immutable_ids_and_a_bounded_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1.0/me/mailFolders/inbox/messages"))
        .and(header("authorization", "Bearer secret"))
        .and(header("prefer", "IdType=\"ImmutableId\""))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"value":[{"id":"one","subject":"Hello"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();
    let page = client.messages("inbox", 10, None).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(!page.truncated);
}

#[tokio::test]
async fn reading_a_message_requests_terminal_friendly_text_content() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1.0/me/messages/message-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id":"message-1",
            "subject":"Hello",
            "body":{"contentType":"text","content":"Readable body"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let message = client.message("message-1").await.unwrap();

    assert_eq!(message["body"]["content"], "Readable body");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests[0].headers.get("prefer").unwrap().to_str().unwrap(),
        "IdType=\"ImmutableId\", outlook.body-content-type=\"text\""
    );
}

#[tokio::test]
async fn send_mail_uses_graphs_expected_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1.0/me/sendMail"))
        .and(body_json(json!({
            "message":{
                "subject":"Hello",
                "body":{"contentType":"Text","content":"Body"},
            "toRecipients":[{"emailAddress":{"address":"person@example.com"}}],
            "ccRecipients":[],
            "bccRecipients":[{"emailAddress":{"address":"private@example.com"}}]
            },
            "saveToSentItems":true
        })))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();
    let value = client
        .send_mail(
            &["person@example.com".into()],
            &[],
            &["private@example.com".into()],
            "Hello",
            "Body",
        )
        .await
        .unwrap();
    assert_eq!(value["sent"], true);
}

#[tokio::test]
async fn search_mail_uses_graph_search_and_keeps_results_bounded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1.0/me/messages"))
        .and(query_param(
            "$search",
            "\"subject:\\\"quarterly report\\\"\"",
        ))
        .and(query_param("$top", "5"))
        .and(header("authorization", "Bearer secret"))
        .and(header("prefer", "IdType=\"ImmutableId\""))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"value":[{"id":"one","subject":"Quarterly report"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let page = client
        .search_messages("subject:\"quarterly report\"", None, 5, None)
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0]["id"], "one");
    assert!(!page.truncated);
}

#[tokio::test]
async fn marking_a_message_updates_only_its_read_state() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/v1.0/me/messages/message-1"))
        .and(header("authorization", "Bearer secret"))
        .and(header("prefer", "IdType=\"ImmutableId\""))
        .and(body_json(json!({"isRead":true})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id":"message-1","isRead":true})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let message = client.set_message_read("message-1", true).await.unwrap();

    assert_eq!(message["id"], "message-1");
    assert_eq!(message["isRead"], true);
}

#[tokio::test]
async fn deleting_a_message_uses_graphs_delete_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v1.0/me/messages/message-1"))
        .and(header("authorization", "Bearer secret"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let result = client.delete_message("message-1").await.unwrap();

    assert_eq!(result, json!({"deleted":true,"message_id":"message-1"}));
}

#[tokio::test]
async fn creating_a_draft_returns_the_saved_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1.0/me/messages"))
        .and(header("authorization", "Bearer secret"))
        .and(header("prefer", "IdType=\"ImmutableId\""))
        .and(body_json(json!({
            "subject":"Hello",
            "body":{"contentType":"Text","content":"Body"},
            "toRecipients":[{"emailAddress":{"address":"to@example.com"}}],
            "ccRecipients":[],
            "bccRecipients":[{"emailAddress":{"address":"bcc@example.com"}}]
        })))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({"id":"draft-1","subject":"Hello","isDraft":true})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let draft = client
        .create_draft(
            &["to@example.com".into()],
            &[],
            &["bcc@example.com".into()],
            "Hello",
            "Body",
        )
        .await
        .unwrap();

    assert_eq!(draft["id"], "draft-1");
    assert_eq!(draft["isDraft"], true);
}

#[tokio::test]
async fn updating_a_draft_changes_only_requested_fields() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/v1.0/me/messages/draft-1"))
        .and(body_json(json!({
            "subject":"Revised",
            "toRecipients":[],
            "body":{"contentType":"Text","content":"New body"}
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":"draft-1","subject":"Revised","isDraft":true})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();
    let empty: Vec<String> = Vec::new();

    let draft = client
        .update_draft(
            "draft-1",
            Some(&empty),
            None,
            None,
            Some("Revised"),
            Some("New body"),
        )
        .await
        .unwrap();

    assert_eq!(draft["subject"], "Revised");
}

#[tokio::test]
async fn sending_a_draft_posts_an_empty_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1.0/me/messages/draft-1/send"))
        .and(body_bytes(Vec::<u8>::new()))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let result = client.send_draft("draft-1").await.unwrap();

    assert_eq!(result, json!({"sent":true,"draft_id":"draft-1"}));
}

#[tokio::test]
async fn adding_a_small_file_attachment_uses_base64_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1.0/me/messages/draft-1/attachments"))
        .and(body_json(json!({
            "@odata.type":"#microsoft.graph.fileAttachment",
            "name":"hello.txt",
            "contentType":"text/plain",
            "contentBytes":"aGVsbG8="
        })))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({"id":"attachment-1","name":"hello.txt","size":5})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let attachment = client
        .add_file_attachment("draft-1", "hello.txt", "text/plain", b"hello")
        .await
        .unwrap();

    assert_eq!(attachment["id"], "attachment-1");
}

#[tokio::test]
async fn adding_a_large_file_attachment_uses_an_upload_session() {
    let server = MockServer::start().await;
    let bytes = vec![b'x'; 3 * 1024 * 1024];
    Mock::given(method("POST"))
        .and(path(
            "/v1.0/me/messages/draft-1/attachments/createUploadSession",
        ))
        .and(body_json(json!({
            "AttachmentItem":{
                "attachmentType":"file",
                "name":"large.bin",
                "size":3145728,
                "contentType":"application/octet-stream"
            }
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "uploadUrl":format!("{}/upload", server.uri()),
            "expirationDateTime":"2026-09-03T12:00:00Z",
            "nextExpectedRanges":["0-"]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/upload"))
        .and(header("content-range", "bytes 0-3145727/3145728"))
        .and(header("content-length", "3145728"))
        .and(body_bytes(bytes.clone()))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({"id":"attachment-2","name":"large.bin","size":3145728})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let attachment = client
        .add_file_attachment("draft-1", "large.bin", "application/octet-stream", &bytes)
        .await
        .unwrap();

    assert_eq!(attachment["id"], "attachment-2");
}

#[tokio::test]
async fn listing_attachments_returns_metadata_without_file_content() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1.0/me/messages/message-1/attachments"))
        .and(query_param(
            "$select",
            "id,name,contentType,size,isInline,lastModifiedDateTime",
        ))
        .and(query_param("$top", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value":[{"id":"attachment-1","name":"report.pdf","size":42}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let page = client.attachments("message-1", 10, None).await.unwrap();

    assert_eq!(page.items[0]["name"], "report.pdf");
}

#[tokio::test]
async fn downloading_an_attachment_returns_its_raw_bytes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/v1.0/me/messages/message-1/attachments/attachment-1/$value",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"file bytes"))
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let bytes = client
        .download_attachment("message-1", "attachment-1")
        .await
        .unwrap();

    assert_eq!(bytes, b"file bytes");
}

#[tokio::test]
async fn deleting_an_attachment_uses_its_message_scoped_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v1.0/me/messages/message-1/attachments/attachment-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let client =
        GraphClient::with_base("secret".into(), &format!("{}/v1.0", server.uri())).unwrap();

    let result = client
        .delete_attachment("message-1", "attachment-1")
        .await
        .unwrap();

    assert_eq!(
        result,
        json!({
            "deleted":true,
            "message_id":"message-1",
            "attachment_id":"attachment-1"
        })
    );
}
