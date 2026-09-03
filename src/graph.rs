use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::AppError;

const GRAPH: &str = "https://graph.microsoft.com/v1.0";
const MESSAGE_FIELDS: &str = "id,subject,from,toRecipients,ccRecipients,receivedDateTime,sentDateTime,isRead,hasAttachments,importance,bodyPreview,body,webLink";
const IMMUTABLE_ID: &str = "IdType=\"ImmutableId\"";

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
        self.get_value(url, false, None).await
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
                .append_pair("$select", MESSAGE_FIELDS)
                .append_pair("$orderby", "receivedDateTime desc")
                .append_pair("$top", &limit.to_string());
            url
        };
        self.get_page(first, limit, None).await
    }

    pub async fn message(&self, id: &str) -> Result<Value, AppError> {
        let mut url = self.endpoint(&format!("me/messages/{}", segment(id)))?;
        url.query_pairs_mut().append_pair("$select", MESSAGE_FIELDS);
        self.get_value(url, true, None).await
    }

    pub async fn send_mail(
        &self,
        to: &[String],
        cc: &[String],
        subject: &str,
        body: &str,
    ) -> Result<Value, AppError> {
        let payload = json!({"message":{
            "subject": subject,
            "body":{"contentType":"Text","content":body},
            "toRecipients": recipients(to),
            "ccRecipients": recipients(cc)
        },"saveToSentItems":true});
        self.post_empty(self.endpoint("me/sendMail")?, payload)
            .await?;
        Ok(json!({"sent":true,"to":to,"cc":cc,"subject":subject}))
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
    ) -> Result<Value, AppError> {
        let mut request = self.http.get(url).bearer_auth(&self.token);
        if immutable {
            request = request.header("Prefer", IMMUTABLE_ID);
        }
        if let Some(timezone) = timezone {
            request = request.header("Prefer", format!("outlook.timezone=\"{timezone}\""));
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
