use {
    crossterm::{
        cursor,
        style::{Color, ResetColor, SetForegroundColor},
        terminal, QueueableCommand,
    },
    once_cell::sync::Lazy,
    parking_lot::{lock_api::ArcMutexGuard, Mutex, RawMutex},
    std::{
        fmt::{Display, Write as _},
        io::{Stdout, Write},
        process,
        sync::Arc,
        time::Duration,
    },
    tokio::{select, signal::ctrl_c, sync::oneshot, task, time::interval},
    tracing::{error, field::Visit, warn, Level, Subscriber},
    tracing_subscriber::Layer,
};

type OptionDynTerm = Option<Box<dyn Term + Send + Sync>>;
static GLOBAL_TERM: Lazy<Arc<Mutex<OptionDynTerm>>> = Lazy::new(|| Arc::new(Mutex::new(None)));

pub struct GlobalTerm(ArcMutexGuard<RawMutex, Option<Box<dyn Term + Send + Sync>>>);

impl Term for GlobalTerm {
    fn set_status(&mut self, status: &str) {
        self.0
            .as_mut()
            .expect("term not initialized")
            .set_status(status);
    }

    fn clear_status(&mut self) {
        self.0
            .as_mut()
            .expect("term not initialized")
            .clear_status();
    }

    fn write(&mut self, level: Level, text: &str) {
        self.0
            .as_mut()
            .expect("term not initialized")
            .write(level, text);
    }
}

pub fn term() -> GlobalTerm {
    GlobalTerm(Mutex::lock_arc(&GLOBAL_TERM))
}

pub fn set_term(term: Option<Box<dyn Term + Send + Sync>>) {
    *GLOBAL_TERM.lock() = term;
}

#[must_use]
pub struct StatusGuard;

impl StatusGuard {
    pub fn set(&self, status: impl Display) {
        term().set_status(&status.to_string());
    }
}

impl Drop for StatusGuard {
    fn drop(&mut self) {
        clear_status()
    }
}

pub fn set_status(status: impl Display) -> StatusGuard {
    term().set_status(&status.to_string());
    StatusGuard
}

pub fn clear_status() {
    term().clear_status()
}

pub struct StatusUpdaterGuard(Option<oneshot::Sender<()>>);

impl Drop for StatusUpdaterGuard {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

pub fn set_status_updater(
    mut updater: impl FnMut() -> String + Send + 'static,
) -> StatusUpdaterGuard {
    let (sender, mut receiver) = oneshot::channel();

    task::spawn(async move {
        let mut interval = interval(Duration::from_secs(1));
        let status = set_status(updater());
        loop {
            select! {
                _ = interval.tick() => {
                    status.set(updater());
                }
                _ = &mut receiver => break,
            }
        }
    });

    StatusUpdaterGuard(Some(sender))
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
        term().write(level, &message);
    }

    fn enabled(
        &self,
        metadata: &tracing::Metadata<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        metadata
            .module_path()
            .is_some_and(|path| path.starts_with("rammingen"))
    }
}

struct DebugVisitor<'a>(&'a mut String, &'a mut Vec<String>);

impl Visit for DebugVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            write!(self.0, "{value:?}").unwrap();
        } else {
            self.1.push(format!("{} = {:?}", field.name(), value));
        }
    }
}

pub trait Term {
    fn set_status(&mut self, status: &str);
    fn clear_status(&mut self);
    fn write(&mut self, level: Level, text: &str);
}

pub struct StdoutTerm {
    stdout: Stdout,
    current_status: Option<String>,
}

impl StdoutTerm {
    pub fn new() -> Self {
        task::spawn(async {
            match ctrl_c().await {
                Ok(()) => {
                    clear_status();
                    error!("Interrupted.");
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
}

impl Default for StdoutTerm {
    fn default() -> Self {
        Self::new()
    }
}

impl Term for StdoutTerm {
    fn set_status(&mut self, status: &str) {
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

    fn write(&mut self, level: Level, text: &str) {
        let color = if level == Level::ERROR || level == Level::WARN {
            Some(Color::Red)
        } else if level == Level::INFO {
            None
        } else {
            Some(Color::Grey)
        };

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
            self.set_status(&old_status);
        }
        self.stdout.flush().unwrap();
    }
}
