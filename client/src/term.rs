use std::fmt::Write as _;
use std::process;
use std::{
    fmt::Display,
    io::{Stdout, Write},
    sync::Arc,
};

use crossterm::{
    cursor,
    style::{Color, ResetColor, SetForegroundColor},
    terminal, QueueableCommand,
};
use once_cell::sync::Lazy;
use parking_lot::{lock_api::ArcMutexGuard, Mutex, RawMutex};
use tokio::signal::ctrl_c;
use tokio::task;
use tracing::{field::Visit, Subscriber};
use tracing::{warn, Level};
use tracing_subscriber::Layer;

struct Term {
    stdout: Stdout,
    current_status: Option<String>,
}

fn term() -> ArcMutexGuard<RawMutex, Term> {
    static TERM: Lazy<Arc<Mutex<Term>>> = Lazy::new(|| Arc::new(Mutex::new(Term::new())));
    Mutex::lock_arc(&TERM)
}

pub fn set_status(status: impl Display) {
    term().set_status(status)
}

pub fn clear_status() {
    term().clear_status()
}

pub fn debug(text: impl Display) {
    term().write(Some(Color::Grey), text)
}

pub fn info(text: impl Display) {
    term().write(None, text)
}

pub fn warn(text: impl Display) {
    term().write(Some(Color::DarkYellow), text)
}

pub fn error(text: impl Display) {
    term().write(Some(Color::Red), text)
}

impl Term {
    fn new() -> Self {
        task::spawn(async {
            match ctrl_c().await {
                Ok(()) => {
                    clear_status();
                    error("Interrupted.");
                    process::exit(1);
                }
                Err(err) => {
                    warn!(?err, "failed to listen to interrupt signal");
                }
            }
        });
        Self {
            stdout: std::io::stdout(),
            current_status: None,
        }
    }

    fn set_status(&mut self, status: impl Display) {
        let status = status.to_string();
        if self.current_status.is_none() {
            self.stdout.queue(cursor::Hide).unwrap();
            self.stdout.queue(terminal::DisableLineWrap).unwrap();
        } else {
            self.stdout.queue(cursor::RestorePosition).unwrap();
            self.stdout
                .queue(terminal::Clear(terminal::ClearType::FromCursorDown))
                .unwrap();
        }
        self.stdout.queue(cursor::SavePosition).unwrap();
        self.stdout
            .queue(SetForegroundColor(Color::DarkGreen))
            .unwrap();
        self.stdout.write_all(status.as_bytes()).unwrap();
        self.stdout.queue(ResetColor).unwrap();
        self.stdout.queue(cursor::RestorePosition).unwrap();
        self.stdout.flush().unwrap();
        self.current_status = Some(status);
    }

    fn clear_status(&mut self) {
        if self.current_status.is_none() {
            return;
        }

        self.stdout.queue(cursor::RestorePosition).unwrap();
        self.stdout
            .queue(terminal::Clear(terminal::ClearType::FromCursorDown))
            .unwrap();
        self.stdout.queue(terminal::EnableLineWrap).unwrap();
        self.stdout.queue(cursor::Show).unwrap();
        self.stdout.flush().unwrap();

        self.current_status = None;
    }

    fn write(&mut self, color: Option<Color>, text: impl Display) {
        let old_status = self.current_status.clone();
        self.clear_status();
        if let Some(color) = color {
            self.stdout.queue(SetForegroundColor(color)).unwrap();
        }
        let mut text = text.to_string();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.stdout.write_all(text.as_bytes()).unwrap();
        if color.is_some() {
            self.stdout.queue(ResetColor).unwrap();
        }
        if let Some(old_status) = old_status {
            self.set_status(old_status);
        }
        self.stdout.flush().unwrap();
    }
}

impl Default for Term {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TermLayer;

impl<S: Subscriber> Layer<S> for TermLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut message = String::new();
        let mut fields = Vec::new();
        event.record(&mut DebugVisitor(&mut message, &mut fields));
        if !fields.is_empty() {
            write!(message, " ({})", fields.join(", ")).unwrap();
        }
        let level = *event.metadata().level();
        if level == Level::ERROR || level == Level::WARN {
            error(message);
        } else if level == Level::INFO {
            info(message);
        } else {
            debug(message);
        }
    }

    fn enabled(
        &self,
        metadata: &tracing::Metadata<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        metadata
            .module_path()
            .map_or(false, |path| path.starts_with("rammingen"))
    }
}

struct DebugVisitor<'a>(&'a mut String, &'a mut Vec<String>);

impl Visit for DebugVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            write!(self.0, "{:?}", value).unwrap();
        } else {
            self.1.push(format!("{} = {:?}", field.name(), value));
        }
    }
}
