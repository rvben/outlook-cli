use outlook_cli::graph::GraphClient;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
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
async fn send_mail_uses_graphs_expected_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1.0/me/sendMail"))
        .and(body_json(json!({
            "message":{
                "subject":"Hello",
                "body":{"contentType":"Text","content":"Body"},
                "toRecipients":[{"emailAddress":{"address":"person@example.com"}}],
                "ccRecipients":[]
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
        .send_mail(&["person@example.com".into()], &[], "Hello", "Body")
        .await
        .unwrap();
    assert_eq!(value["sent"], true);
}
