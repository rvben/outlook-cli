//! Shared read operations. Graph-only operations remain on GraphClient.
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
