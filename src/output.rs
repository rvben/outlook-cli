use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};

static NO_COLOR: AtomicBool = AtomicBool::new(false);

pub fn set_no_color(disabled: bool) {
    NO_COLOR.store(disabled, Ordering::Relaxed);
}

pub fn accent(text: &str) -> String {
    if std::io::stdout().is_terminal()
        && !NO_COLOR.load(Ordering::Relaxed)
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").as_deref() != Ok("dumb")
    {
        format!("\x1b[1;36m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

use clap::ValueEnum;
use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Auto,
    Text,
    Json,
}

#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub format: OutputFormat,
    pub quiet: bool,
}

impl Output {
    pub fn json(self) -> bool {
        matches!(self.format, OutputFormat::Json)
            || (matches!(self.format, OutputFormat::Auto) && !std::io::stdout().is_terminal())
    }

    pub fn value<T: Serialize>(
        &self,
        value: &T,
        text: impl FnOnce() -> String,
    ) -> Result<(), AppError> {
        if self.json() {
            println!(
                "{}",
                serde_json::to_string_pretty(value)
                    .map_err(|error| AppError::Unexpected(error.to_string()))?
            );
        } else {
            println!("{}", text());
        }
        Ok(())
    }

    pub fn note(&self, message: impl AsRef<str>) {
        if !self.quiet {
            eprintln!("{}", message.as_ref());
        }
    }
}

pub fn print_error(error: &AppError, structured: bool) {
    if structured {
        let contract = error.contract();
        let envelope = serde_json::json!({"error":{"kind":contract.kind,"message":error.to_string(),"retryable":contract.retryable}});
        eprintln!("{}", serde_json::to_string(&envelope).unwrap_or_else(|_| "{\"error\":{\"kind\":\"unexpected_error\",\"message\":\"error serialization failed\",\"retryable\":false}}".into()));
    } else {
        eprintln!("Error: {error}");
        eprintln!("Hint: run `outlook doctor` for a guided diagnosis.");
    }
}

pub fn structured_from_args() -> bool {
    let args: Vec<String> = std::env::args().take_while(|arg| arg != "--").collect();
    let explicit_text = args
        .windows(2)
        .any(|pair| pair == ["--output", "text"] || pair == ["-o", "text"])
        || args
            .iter()
            .any(|arg| arg == "--output=text" || arg == "-otext");
    let explicit_json = args
        .windows(2)
        .any(|pair| pair == ["--output", "json"] || pair == ["-o", "json"])
        || args
            .iter()
            .any(|arg| arg == "--output=json" || arg == "-ojson" || arg == "--json");
    !explicit_text && (explicit_json || !std::io::stdout().is_terminal())
}
