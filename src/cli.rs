use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;

use crate::output::OutputFormat;

#[derive(Debug, Parser)]
#[command(
    name = "outlook",
    version,
    about = "Microsoft Outlook from your terminal, for humans and agents",
    after_help = "Get started:\n  outlook init\n  outlook inbox\n  outlook calendar agenda --start START --end END\n  outlook doctor\n  outlook schema --command 'mail send'"
)]
pub struct Cli {
    #[arg(long, global = true, env = "OUTLOOK_PROFILE")]
    pub profile: Option<String>,
    #[arg(short = 'o', long, global = true, value_enum, default_value = "auto")]
    pub output: OutputFormat,
    #[arg(long, global = true)]
    pub quiet: bool,
    /// Skip confirmation prompts for destructive operations
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,
    #[arg(long, global = true, hide = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Configure an Entra public-client app and optionally sign in
    Init(InitArgs),
    /// Manage delegated Microsoft authentication
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// List, select, or remove configuration profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Inspect resolved secret-free configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Show the signed-in Microsoft identity
    Whoami,
    /// List recent messages in the inbox
    Inbox(PageArgs),
    /// Read and manage email
    Mail {
        #[command(subcommand)]
        command: MailCommand,
    },
    /// View and manage calendars
    Calendar {
        #[command(subcommand)]
        command: CalendarCommand,
    },
    /// Diagnose configuration, credentials, and Graph access
    Doctor {
        #[arg(long)]
        offline: bool,
    },
    /// Describe supported and planned capabilities
    Capabilities,
    /// Emit the offline CLI Spec v0.3 contract
    Schema {
        #[arg(long)]
        command: Option<String>,
    },
    /// Generate shell completions
    Completions { shell: Shell },
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Override the bundled Microsoft Entra public-client application ID
    #[arg(long, env = "OUTLOOK_CLIENT_ID")]
    pub client_id: Option<String>,
    /// Tenant ID/domain, common, organizations, or consumers
    #[arg(long, default_value = "common", env = "OUTLOOK_TENANT")]
    pub tenant: String,
    /// Save configuration without signing in
    #[arg(long)]
    pub no_login: bool,
    /// Request only read scopes and block remote writes
    #[arg(long, env = "OUTLOOK_READ_ONLY")]
    pub read_only: bool,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Sign in using Microsoft's device-code flow
    Login,
    /// Remove locally stored credentials
    Logout,
    /// Verify authentication and show the selected profile's status
    Status {
        /// Inspect local credential state without contacting Microsoft Graph
        #[arg(long)]
        offline: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// List configured profiles and identify the active one
    List,
    /// Select the default profile for future commands
    Use { name: String },
    /// Remove a profile and its stored credential
    Remove { name: String },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Show,
    Path,
}

#[derive(Debug, Args, Clone)]
pub struct PageArgs {
    /// Maximum records in this page
    #[arg(long, default_value_t = 25, value_parser = clap::value_parser!(u16).range(1..=100))]
    pub limit: u16,
    /// Opaque continuation URL from a previous response
    #[arg(long)]
    pub cursor: Option<String>,
    /// Comma-separated output fields
    #[arg(long)]
    pub fields: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum MailCommand {
    /// List messages in a well-known folder or folder ID
    List {
        #[arg(long, default_value = "inbox")]
        folder: String,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Read one message
    Read { id: String },
    /// Search messages using Microsoft Outlook search syntax
    Search {
        /// Search text or a supported KQL expression
        query: String,
        /// Restrict the search to a well-known folder or folder ID
        #[arg(long)]
        folder: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Mark one message as read
    MarkRead { id: String },
    /// Mark one message as unread
    MarkUnread { id: String },
    /// Send a plain-text message
    Send {
        #[arg(long, required = true)]
        to: Vec<String>,
        #[arg(long)]
        cc: Vec<String>,
        #[arg(long)]
        bcc: Vec<String>,
        #[arg(long)]
        subject: String,
        /// Plain-text body, or - to read stdin
        #[arg(long)]
        body: String,
    },
    /// Reply to one message
    Reply {
        id: String,
        /// Plain-text comment, or - to read stdin
        #[arg(long)]
        body: String,
        #[arg(long)]
        all: bool,
    },
    /// Move a message to a well-known folder or folder ID
    Move {
        id: String,
        #[arg(long)]
        destination: String,
    },
    /// Delete one message after confirmation
    Delete { id: String },
    /// Create, inspect, update, send, or delete drafts
    Draft {
        #[command(subcommand)]
        command: DraftCommand,
    },
    /// List, add, download, or delete message attachments
    Attachment {
        #[command(subcommand)]
        command: AttachmentCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AttachmentCommand {
    /// List attachment metadata without downloading content
    List {
        message_id: String,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Add a file attachment to a draft
    Add {
        message_id: String,
        path: PathBuf,
        /// Override the attachment MIME type
        #[arg(long)]
        content_type: Option<String>,
    },
    /// Download an attachment without overwriting by default
    Download {
        message_id: String,
        attachment_id: String,
        path: PathBuf,
        /// Replace an existing destination file
        #[arg(long)]
        force: bool,
    },
    /// Delete an attachment after confirmation
    Delete {
        message_id: String,
        attachment_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum DraftCommand {
    /// List messages in the Drafts folder
    List(PageArgs),
    /// Create a saved plain-text draft
    Create {
        #[arg(long)]
        to: Vec<String>,
        #[arg(long)]
        cc: Vec<String>,
        #[arg(long)]
        bcc: Vec<String>,
        #[arg(long, default_value = "")]
        subject: String,
        /// Plain-text body, or - to read stdin
        #[arg(long, default_value = "")]
        body: String,
    },
    /// Update selected fields on a saved draft
    Update {
        id: String,
        #[arg(long, conflicts_with = "clear_to")]
        to: Vec<String>,
        #[arg(long)]
        clear_to: bool,
        #[arg(long, conflicts_with = "clear_cc")]
        cc: Vec<String>,
        #[arg(long)]
        clear_cc: bool,
        #[arg(long, conflicts_with = "clear_bcc")]
        bcc: Vec<String>,
        #[arg(long)]
        clear_bcc: bool,
        #[arg(long)]
        subject: Option<String>,
        /// Plain-text body, or - to read stdin
        #[arg(long)]
        body: Option<String>,
    },
    /// Send an existing draft
    Send { id: String },
    /// Delete an existing draft after confirmation
    Delete { id: String },
}

#[derive(Debug, Subcommand)]
pub enum CalendarCommand {
    /// List occurrences and events in a time range
    Agenda {
        /// Inclusive ISO 8601 start date-time
        #[arg(long)]
        start: String,
        /// Exclusive ISO 8601 end date-time
        #[arg(long)]
        end: String,
        /// Windows or IANA timezone requested for returned event times
        #[arg(long, default_value = "UTC")]
        timezone: String,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Create an event in the default calendar
    Create {
        #[arg(long)]
        subject: String,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long, default_value = "UTC")]
        timezone: String,
        #[arg(long)]
        attendee: Vec<String>,
        #[arg(long, default_value = "")]
        body: String,
    },
}
