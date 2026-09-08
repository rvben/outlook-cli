//! Shared mail operations. Graph-only operations remain on GraphClient.
use crate::{
    auth,
    config::{self, BackendKind},
    desktop::DesktopClient,
    error::AppError,
    graph::{GraphClient, Page},
};
use serde_json::Value;

pub enum MailBackend {
    Graph(GraphClient),
    Desktop(DesktopClient),
}
impl MailBackend {
    pub async fn connect_writable(profile: Option<&str>) -> Result<Self, AppError> {
        let (_, selected) = config::load(profile)?;
        selected.require_writable()?;
        Self::connect(profile).await
    }
    pub async fn delete_draft(&self, id: &str) -> Result<Value, AppError> {
        match self {
            Self::Graph(client) => client.delete_message(id).await,
            Self::Desktop(client) => client.delete_draft(id).await,
        }
    }
    pub async fn send_mail(
        &self,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        subject: &str,
        body: &str,
    ) -> Result<Value, AppError> {
        match self {
            Self::Graph(client) => client.send_mail(to, cc, bcc, subject, body).await,
            Self::Desktop(client) => client.send_mail(to, cc, bcc, subject, body).await,
        }
    }
    pub async fn create_draft(
        &self,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        subject: &str,
        body: &str,
    ) -> Result<Value, AppError> {
        match self {
            Self::Graph(client) => client.create_draft(to, cc, bcc, subject, body).await,
            Self::Desktop(client) => client.create_draft(to, cc, bcc, subject, body).await,
        }
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
        match self {
            Self::Graph(client) => client.update_draft(id, to, cc, bcc, subject, body).await,
            Self::Desktop(client) => client.update_draft(id, to, cc, bcc, subject, body).await,
        }
    }
    pub async fn send_draft(&self, id: &str) -> Result<Value, AppError> {
        match self {
            Self::Graph(client) => client.send_draft(id).await,
            Self::Desktop(client) => client.send_draft(id).await,
        }
    }
    pub async fn reply(&self, id: &str, body: &str, all: bool) -> Result<Value, AppError> {
        match self {
            Self::Graph(client) => client.reply(id, body, all).await,
            Self::Desktop(client) => client.reply(id, body, all).await,
        }
    }
    pub async fn move_message(&self, id: &str, destination: &str) -> Result<Value, AppError> {
        match self {
            Self::Graph(client) => client.move_message(id, destination).await,
            Self::Desktop(client) => client.move_message(id, destination).await,
        }
    }
    pub async fn set_message_read(&self, id: &str, read: bool) -> Result<Value, AppError> {
        match self {
            Self::Graph(client) => client.set_message_read(id, read).await,
            Self::Desktop(client) => client.set_message_read(id, read).await,
        }
    }
    pub async fn delete_message(&self, id: &str) -> Result<Value, AppError> {
        match self {
            Self::Graph(client) => client.delete_message(id).await,
            Self::Desktop(client) => client.delete_message(id).await,
        }
    }

    pub async fn connect(profile: Option<&str>) -> Result<Self, AppError> {
        let (name, profile) = config::load(profile)?;
        match profile.backend {
            BackendKind::Graph => Ok(Self::Graph(GraphClient::new(
                auth::access_token(&name, &profile).await?,
            ))),
            BackendKind::Desktop => Ok(Self::Desktop(DesktopClient)),
        }
    }
    pub async fn messages(
        &self,
        folder: &str,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<Page, AppError> {
        match self {
            Self::Graph(client) => client.messages(folder, limit, cursor).await,
            Self::Desktop(client) => client.page("list", Some(folder), None, limit, cursor).await,
        }
    }
    pub async fn message(&self, id: &str) -> Result<Value, AppError> {
        match self {
            Self::Graph(client) => client.message(id).await,
            Self::Desktop(client) => client.message(id).await,
        }
    }
    pub async fn search_messages(
        &self,
        query: &str,
        folder: Option<&str>,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<Page, AppError> {
        match self {
            Self::Graph(client) => client.search_messages(query, folder, limit, cursor).await,
            Self::Desktop(client) => {
                client
                    .page(
                        "search",
                        Some(folder.unwrap_or("inbox")),
                        Some(query),
                        limit,
                        cursor,
                    )
                    .await
            }
        }
    }
    pub async fn folders(
        &self,
        parent: Option<&str>,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<Page, AppError> {
        match self {
            Self::Graph(client) => client.folders(parent, limit, cursor).await,
            Self::Desktop(client) => client.page("folders", parent, None, limit, cursor).await,
        }
    }
}
