use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::AppError;

const GRAPH: &str = "https://graph.microsoft.com/v1.0";
const MESSAGE_SUMMARY_FIELDS: &str = "id,subject,from,toRecipients,ccRecipients,bccRecipients,receivedDateTime,sentDateTime,isRead,isDraft,hasAttachments,importance,bodyPreview,webLink";
const MESSAGE_DETAIL_FIELDS: &str = "id,subject,from,toRecipients,ccRecipients,bccRecipients,receivedDateTime,sentDateTime,isRead,isDraft,hasAttachments,importance,bodyPreview,body,webLink";
const IMMUTABLE_ID: &str = "IdType=\"ImmutableId\"";
const SMALL_ATTACHMENT_LIMIT: usize = 3 * 1024 * 1024;
pub const MAX_ATTACHMENT_SIZE: usize = 150 * 1024 * 1024;
const UPLOAD_CHUNK_SIZE: usize = 10 * 320 * 1024;

#[derive(Debug, Serialize)]
pub struct Page {
    pub items: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
struct GraphPage {
    value: Vec<Value>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

pub struct GraphClient {
    http: reqwest::Client,
    token: String,
    base: url::Url,
}

impl GraphClient {
    pub fn new(token: String) -> Self {
        Self::with_base(token, GRAPH).expect("the Microsoft Graph base URL is valid")
    }

    pub fn with_base(token: String, base: &str) -> Result<Self, AppError> {
        let base = url::Url::parse(&format!("{}/", base.trim_end_matches('/')))
            .map_err(|error| AppError::InvalidInput(format!("invalid Graph base URL: {error}")))?;
        Ok(Self {
            http: reqwest::Client::new(),
            token,
            base,
        })
    }

    pub async fn me(&self) -> Result<Value, AppError> {
        let mut url = self.endpoint("me")?;
        url.query_pairs_mut()
            .append_pair("$select", "id,displayName,userPrincipalName,mail");
        self.get_value(url, false, None, false).await
    }

    pub async fn messages(
        &self,
        folder: &str,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<Page, AppError> {
        let first = if let Some(cursor) = cursor {
            self.cursor(cursor)?
        } else {
            let mut url = self.endpoint(&format!("me/mailFolders/{}/messages", segment(folder)))?;
            url.query_pairs_mut()
                .append_pair("$select", MESSAGE_SUMMARY_FIELDS)
                .append_pair("$orderby", "receivedDateTime desc")
                .append_pair("$top", &limit.to_string());
            url
        };
        self.get_page(first, limit, None).await
    }

    pub async fn message(&self, id: &str) -> Result<Value, AppError> {
        let mut url = self.endpoint(&format!("me/messages/{}", segment(id)))?;
        url.query_pairs_mut()
            .append_pair("$select", MESSAGE_DETAIL_FIELDS);
        self.get_value(url, true, None, true).await
    }

    pub async fn search_messages(
        &self,
        query: &str,
        folder: Option<&str>,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<Page, AppError> {
        let first = if let Some(cursor) = cursor {
            self.cursor(cursor)?
        } else {
            let path = match folder {
                Some(folder) => format!("me/mailFolders/{}/messages", segment(folder)),
                None => "me/messages".into(),
            };
            let mut url = self.endpoint(&path)?;
            let escaped = query.replace('\\', "\\\\").replace('"', "\\\"");
            url.query_pairs_mut()
                .append_pair("$search", &format!("\"{escaped}\""))
                .append_pair("$select", MESSAGE_SUMMARY_FIELDS)
                .append_pair("$top", &limit.to_string());
            url
        };
        self.get_page(first, limit, None).await
    }

    pub async fn send_mail(
        &self,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        subject: &str,
        body: &str,
    ) -> Result<Value, AppError> {
        let payload = json!({"message":{
            "subject": subject,
            "body":{"contentType":"Text","content":body},
            "toRecipients": recipients(to),
            "ccRecipients": recipients(cc),
            "bccRecipients": recipients(bcc)
        },"saveToSentItems":true});
        self.post_empty(self.endpoint("me/sendMail")?, payload)
            .await?;
        Ok(json!({"sent":true,"to":to,"cc":cc,"bcc":bcc,"subject":subject}))
    }

    pub async fn create_draft(
        &self,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        subject: &str,
        body: &str,
    ) -> Result<Value, AppError> {
        let payload = json!({
            "subject":subject,
            "body":{"contentType":"Text","content":body},
            "toRecipients":recipients(to),
            "ccRecipients":recipients(cc),
            "bccRecipients":recipients(bcc)
        });
        self.post_value(self.endpoint("me/messages")?, payload, true)
            .await
    }

    pub async fn update_draft(
        &self,
        id: &str,
        to: Option<&[String]>,
        cc: Option<&[String]>,
        bcc: Option<&[String]>,
        subject: Option<&str>,
        body: Option<&str>,
    ) -> Result<Value, AppError> {
        let mut payload = serde_json::Map::new();
        if let Some(to) = to {
            payload.insert("toRecipients".into(), Value::Array(recipients(to)));
        }
        if let Some(cc) = cc {
            payload.insert("ccRecipients".into(), Value::Array(recipients(cc)));
        }
        if let Some(bcc) = bcc {
            payload.insert("bccRecipients".into(), Value::Array(recipients(bcc)));
        }
        if let Some(subject) = subject {
            payload.insert("subject".into(), Value::String(subject.into()));
        }
        if let Some(body) = body {
            payload.insert("body".into(), json!({"contentType":"Text","content":body}));
        }
        self.patch_value(
            self.endpoint(&format!("me/messages/{}", segment(id)))?,
            Value::Object(payload),
            true,
        )
        .await
    }

    pub async fn send_draft(&self, id: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .post(self.endpoint(&format!("me/messages/{}/send", segment(id)))?)
            .bearer_auth(&self.token)
            .header("Prefer", IMMUTABLE_ID)
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .body(Vec::new())
            .send()
            .await
            .map_err(unreachable)?;
        self.checked(response).await?;
        Ok(json!({"sent":true,"draft_id":id}))
    }

    pub async fn add_file_attachment(
        &self,
        message_id: &str,
        name: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<Value, AppError> {
        if bytes.len() > MAX_ATTACHMENT_SIZE {
            return Err(AppError::InvalidInput(
                "attachments cannot exceed 150 MiB".into(),
            ));
        }
        if bytes.len() >= SMALL_ATTACHMENT_LIMIT {
            return self
                .upload_large_attachment(message_id, name, content_type, bytes)
                .await;
        }
        let payload = json!({
            "@odata.type":"#microsoft.graph.fileAttachment",
            "name":name,
            "contentType":content_type,
            "contentBytes":base64::engine::general_purpose::STANDARD.encode(bytes)
        });
        self.post_value(
            self.endpoint(&format!("me/messages/{}/attachments", segment(message_id)))?,
            payload,
            false,
        )
        .await
    }

    pub async fn attachments(
        &self,
        message_id: &str,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<Page, AppError> {
        let first = if let Some(cursor) = cursor {
            self.cursor(cursor)?
        } else {
            let mut url =
                self.endpoint(&format!("me/messages/{}/attachments", segment(message_id)))?;
            url.query_pairs_mut()
                .append_pair(
                    "$select",
                    "id,name,contentType,size,isInline,lastModifiedDateTime",
                )
                .append_pair("$top", &limit.to_string());
            url
        };
        self.get_page(first, limit, None).await
    }

    pub async fn download_attachment(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, AppError> {
        let response = self
            .http
            .get(self.endpoint(&format!(
                "me/messages/{}/attachments/{}/$value",
                segment(message_id),
                segment(attachment_id)
            ))?)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(unreachable)?;
        self.checked(response)
            .await?
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(unreachable)
    }

    pub async fn delete_attachment(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<Value, AppError> {
        let response = self
            .http
            .delete(self.endpoint(&format!(
                "me/messages/{}/attachments/{}",
                segment(message_id),
                segment(attachment_id)
            ))?)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(unreachable)?;
        self.checked(response).await?;
        Ok(json!({
            "deleted":true,
            "message_id":message_id,
            "attachment_id":attachment_id
        }))
    }

    async fn upload_large_attachment(
        &self,
        message_id: &str,
        name: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<Value, AppError> {
        let session = self
            .post_value(
                self.endpoint(&format!(
                    "me/messages/{}/attachments/createUploadSession",
                    segment(message_id)
                ))?,
                json!({"AttachmentItem":{
                    "attachmentType":"file",
                    "name":name,
                    "size":bytes.len(),
                    "contentType":content_type
                }}),
                false,
            )
            .await?;
        let raw_url = session
            .pointer("/uploadUrl")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::Api("upload session did not include uploadUrl".into()))?;
        let upload_url = url::Url::parse(raw_url)
            .map_err(|error| AppError::Api(format!("invalid attachment upload URL: {error}")))?;
        let same_test_origin = upload_url.scheme() == self.base.scheme()
            && upload_url.host_str() == self.base.host_str()
            && upload_url.port_or_known_default() == self.base.port_or_known_default();
        if upload_url.scheme() != "https" && !same_test_origin {
            return Err(AppError::Api("attachment upload URL must use HTTPS".into()));
        }

        let mut final_value = None;
        for (index, chunk) in bytes.chunks(UPLOAD_CHUNK_SIZE).enumerate() {
            let start = index * UPLOAD_CHUNK_SIZE;
            let end = start + chunk.len() - 1;
            let response = self
                .http
                .put(upload_url.clone())
                .header("Content-Length", chunk.len())
                .header(
                    "Content-Range",
                    format!("bytes {start}-{end}/{}", bytes.len()),
                )
                .body(chunk.to_vec())
                .send()
                .await
                .map_err(unreachable)?;
            let status = response.status();
            let response = self.checked(response).await?;
            if status != reqwest::StatusCode::ACCEPTED {
                final_value = Some(response.json().await.map_err(|error| {
                    AppError::Api(format!("invalid attachment upload response: {error}"))
                })?);
            }
        }
        final_value.ok_or_else(|| {
            AppError::Api("attachment upload ended without a completed attachment".into())
        })
    }

    pub async fn reply(&self, id: &str, body: &str, all: bool) -> Result<Value, AppError> {
        let action = if all { "replyAll" } else { "reply" };
        self.post_empty(
            self.endpoint(&format!("me/messages/{}/{action}", segment(id)))?,
            json!({"comment":body}),
        )
        .await?;
        Ok(json!({"sent":true,"message_id":id,"reply_all":all}))
    }

    pub async fn move_message(&self, id: &str, destination: &str) -> Result<Value, AppError> {
        self.post_value(
            self.endpoint(&format!("me/messages/{}/move", segment(id)))?,
            json!({"destinationId":destination}),
            true,
        )
        .await
    }

    pub async fn set_message_read(&self, id: &str, read: bool) -> Result<Value, AppError> {
        self.patch_value(
            self.endpoint(&format!("me/messages/{}", segment(id)))?,
            json!({"isRead":read}),
            true,
        )
        .await
    }

    pub async fn delete_message(&self, id: &str) -> Result<Value, AppError> {
        let response = self
            .http
            .delete(self.endpoint(&format!("me/messages/{}", segment(id)))?)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(unreachable)?;
        self.checked(response).await?;
        Ok(json!({"deleted":true,"message_id":id}))
    }

    pub async fn agenda(
        &self,
        start: &str,
        end: &str,
        timezone: &str,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<Page, AppError> {
        let first = if let Some(cursor) = cursor {
            self.cursor(cursor)?
        } else {
            let mut url = self.endpoint("me/calendarView")?;
            url.query_pairs_mut()
                .append_pair("startDateTime", start)
                .append_pair("endDateTime", end)
                .append_pair("$select", "id,subject,start,end,location,organizer,attendees,isAllDay,isCancelled,webLink")
                .append_pair("$orderby", "start/dateTime")
                .append_pair("$top", &limit.to_string());
            url
        };
        self.get_page(first, limit, Some(timezone)).await
    }

    pub async fn create_event(
        &self,
        subject: &str,
        start: &str,
        end: &str,
        timezone: &str,
        attendees: &[String],
        body: &str,
    ) -> Result<Value, AppError> {
        let payload = json!({
            "subject":subject,
            "body":{"contentType":"Text","content":body},
            "start":{"dateTime":start,"timeZone":timezone},
            "end":{"dateTime":end,"timeZone":timezone},
            "attendees":attendees.iter().map(|address| json!({"emailAddress":{"address":address},"type":"required"})).collect::<Vec<_>>()
        });
        self.post_value(self.endpoint("me/events")?, payload, true)
            .await
    }

    fn endpoint(&self, relative: &str) -> Result<url::Url, AppError> {
        self.base
            .join(relative)
            .map_err(|error| AppError::Unexpected(error.to_string()))
    }

    fn cursor(&self, raw: &str) -> Result<url::Url, AppError> {
        let url = url::Url::parse(raw).map_err(|_| {
            AppError::InvalidInput(
                "--cursor must be an opaque Microsoft Graph continuation URL".into(),
            )
        })?;
        if url.scheme() != self.base.scheme()
            || url.host_str() != self.base.host_str()
            || url.port_or_known_default() != self.base.port_or_known_default()
            || !url.path().starts_with(self.base.path())
        {
            return Err(AppError::InvalidInput(
                "--cursor must be an opaque Microsoft Graph continuation URL".into(),
            ));
        }
        Ok(url)
    }

    async fn get_page(
        &self,
        url: url::Url,
        limit: u16,
        timezone: Option<&str>,
    ) -> Result<Page, AppError> {
        let mut request = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .header("Prefer", IMMUTABLE_ID);
        if let Some(timezone) = timezone {
            request = request.header(
                "Prefer",
                format!("{IMMUTABLE_ID}, outlook.timezone=\"{timezone}\""),
            );
        }
        let response = request.send().await.map_err(unreachable)?;
        let mut page: GraphPage = self
            .checked(response)
            .await?
            .json()
            .await
            .map_err(|error| AppError::Api(format!("invalid Graph response: {error}")))?;
        let over_limit = page.value.len() > limit as usize;
        page.value.truncate(limit as usize);
        let truncated = over_limit || page.next_link.is_some();
        Ok(Page {
            items: page.value,
            next_cursor: page.next_link,
            truncated,
        })
    }

    async fn get_value(
        &self,
        url: url::Url,
        immutable: bool,
        timezone: Option<&str>,
        text_body: bool,
    ) -> Result<Value, AppError> {
        let mut request = self.http.get(url).bearer_auth(&self.token);
        let mut preferences = Vec::new();
        if immutable {
            preferences.push(IMMUTABLE_ID.to_owned());
        }
        if let Some(timezone) = timezone {
            preferences.push(format!("outlook.timezone=\"{timezone}\""));
        }
        if text_body {
            preferences.push("outlook.body-content-type=\"text\"".into());
        }
        if !preferences.is_empty() {
            request = request.header("Prefer", preferences.join(", "));
        }
        let response = request.send().await.map_err(unreachable)?;
        self.checked(response)
            .await?
            .json()
            .await
            .map_err(|error| AppError::Api(format!("invalid Graph response: {error}")))
    }

    async fn post_empty(&self, url: url::Url, payload: Value) -> Result<(), AppError> {
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .header("Prefer", IMMUTABLE_ID)
            .json(&payload)
            .send()
            .await
            .map_err(unreachable)?;
        self.checked(response).await.map(|_| ())
    }

    async fn post_value(
        &self,
        url: url::Url,
        payload: Value,
        immutable: bool,
    ) -> Result<Value, AppError> {
        let mut request = self.http.post(url).bearer_auth(&self.token).json(&payload);
        if immutable {
            request = request.header("Prefer", IMMUTABLE_ID);
        }
        let response = request.send().await.map_err(unreachable)?;
        self.checked(response)
            .await?
            .json()
            .await
            .map_err(|error| AppError::Api(format!("invalid Graph response: {error}")))
    }

    async fn patch_value(
        &self,
        url: url::Url,
        payload: Value,
        immutable: bool,
    ) -> Result<Value, AppError> {
        let mut request = self.http.patch(url).bearer_auth(&self.token).json(&payload);
        if immutable {
            request = request.header("Prefer", IMMUTABLE_ID);
        }
        let response = request.send().await.map_err(unreachable)?;
        self.checked(response)
            .await?
            .json()
            .await
            .map_err(|error| AppError::Api(format!("invalid Graph response: {error}")))
    }

    async fn checked(&self, response: reqwest::Response) -> Result<reqwest::Response, AppError> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        if status.as_u16() == 429 {
            return Err(AppError::RateLimit(retry_after));
        }
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let message = body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Microsoft Graph rejected the request");
        match status.as_u16() {
            401 => Err(AppError::Auth(format!(
                "{message}; run `outlook auth login`"
            ))),
            403 => Err(AppError::Permission(format!(
                "{message}; check delegated scopes and tenant consent with `outlook doctor`"
            ))),
            404 => Err(AppError::NotFound(message.into())),
            _ => Err(AppError::Api(format!("{message} ({status})"))),
        }
    }
}

fn recipients(addresses: &[String]) -> Vec<Value> {
    addresses
        .iter()
        .map(|address| json!({"emailAddress":{"address":address}}))
        .collect()
}

fn segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn unreachable(error: reqwest::Error) -> AppError {
    AppError::Unexpected(format!("Microsoft Graph is unreachable: {error}"))
}

pub fn select_fields(page: &mut Page, fields: Option<&str>) -> Result<(), AppError> {
    let Some(fields) = fields else {
        return Ok(());
    };
    let names: Vec<&str> = fields
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() {
        return Err(AppError::InvalidInput(
            "--fields must name at least one field".into(),
        ));
    }
    for item in &mut page.items {
        let source = item
            .as_object()
            .ok_or_else(|| AppError::Api("Graph returned a non-object collection item".into()))?;
        *item = Value::Object(
            names
                .iter()
                .filter_map(|name| {
                    source
                        .get(*name)
                        .cloned()
                        .map(|value| ((*name).into(), value))
                })
                .collect(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_must_stay_on_graph_origin_and_path() {
        let client = GraphClient::new("token".into());
        assert!(
            client
                .cursor("https://graph.microsoft.com/v1.0/me/messages?$skiptoken=x")
                .is_ok()
        );
        assert!(
            client
                .cursor("https://graph.microsoft.com.evil.test/v1.0/me/messages")
                .is_err()
        );
        assert!(
            client
                .cursor("http://graph.microsoft.com/v1.0/me/messages")
                .is_err()
        );
        assert!(
            client
                .cursor("https://graph.microsoft.com/beta/me/messages")
                .is_err()
        );
    }

    #[test]
    fn ids_are_encoded_as_single_path_segments() {
        assert_eq!(segment("abc/def+ghi"), "abc%2Fdef%2Bghi");
    }
}
