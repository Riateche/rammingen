use {
    crossterm::{
        QueueableCommand, cursor,
        style::{Color, ResetColor, SetForegroundColor},
        terminal,
    },
    parking_lot::{Mutex, RawMutex, lock_api::ArcMutexGuard},
    std::{
        fmt::{Debug, Display, Write as _},
        io::{Stdout, Write, stdout},
        process,
        sync::{Arc, LazyLock},
        time::Duration,
    },
    tokio::{select, signal::ctrl_c, sync::oneshot, task, time::interval},
    tracing::{
        Level, Subscriber, error,
        field::{Field, Visit},
        warn,
    },
    tracing_subscriber::Layer,
};

type OptionDynTerm = Option<Box<dyn Term + Send + Sync>>;
static GLOBAL_TERM: LazyLock<Arc<Mutex<OptionDynTerm>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));

pub struct GlobalTerm(ArcMutexGuard<RawMutex, Option<Box<dyn Term + Send + Sync>>>);

impl Term for GlobalTerm {
    /// # Panics
    ///
    /// Panics if global term is uninitialized.
    #[inline]
    #[expect(clippy::expect_used, reason = "intended")]
    fn set_status(&mut self, status: &str) {
        self.0
            .as_mut()
            .expect("term not initialized")
            .set_status(status);
    }

    /// # Panics
    ///
    /// Panics if global term is uninitialized.
    #[inline]
    #[expect(clippy::expect_used, reason = "intended")]
    fn clear_status(&mut self) {
        self.0
            .as_mut()
            .expect("term not initialized")
            .clear_status();
    }

    /// # Panics
    ///
    /// Panics if global term is uninitialized.
    #[inline]
    #[expect(clippy::expect_used, reason = "intended")]
    fn write(&mut self, level: Level, text: &str) {
        self.0
            .as_mut()
            .expect("term not initialized")
            .write(level, text);
    }
}

#[must_use]
#[inline]
pub fn term() -> GlobalTerm {
    GlobalTerm(Mutex::lock_arc(&GLOBAL_TERM))
}

#[inline]
pub fn set_term(term: Option<Box<dyn Term + Send + Sync>>) {
    *GLOBAL_TERM.lock() = term;
}

#[must_use]
pub struct StatusGuard;

impl StatusGuard {
    #[inline]
    pub fn set(&self, status: impl Display) {
        term().set_status(&status.to_string());
    }
}

impl Drop for StatusGuard {
    #[inline]
    fn drop(&mut self) {
        clear_status();
    }
}

#[inline]
pub fn set_status(status: impl Display) -> StatusGuard {
    term().set_status(&status.to_string());
    StatusGuard
}

#[inline]
pub fn clear_status() {
    term().clear_status();
}

pub struct StatusUpdaterGuard(Option<oneshot::Sender<()>>);

impl Drop for StatusUpdaterGuard {
    #[inline]
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[inline]
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

#[expect(clippy::absolute_paths, reason = "for clarity")]
impl<S: Subscriber> Layer<S> for TermLayer {
    #[inline]
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut message = String::new();
        let mut fields = Vec::new();
        event.record(&mut DebugVisitor(&mut message, &mut fields));
        #[expect(clippy::expect_used, reason = "write to string never fails")]
        if !fields.is_empty() {
            write!(message, " ({})", fields.join(", ")).expect("write to string failed");
        }
        let level = *event.metadata().level();
        term().write(level, &message);
    }

    #[inline]
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
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        #[expect(clippy::expect_used, reason = "write to string never fails")]
        if field.name() == "message" {
            write!(self.0, "{value:?}").expect("write to string failed");
        } else {
            self.1.push(format!("{} = {:?}", field.name(), value));
        }
    }
}

pub trait Term {
    /// # Panics
    ///
    /// Panics if there was an error in the underlying terminal implementation.
    fn set_status(&mut self, status: &str);
    /// # Panics
    ///
    /// Panics if there was an error in the underlying terminal implementation.
    fn clear_status(&mut self);
    /// # Panics
    ///
    /// Panics if there was an error in the underlying terminal implementation.
    fn write(&mut self, level: Level, text: &str);
}

pub struct StdoutTerm {
    stdout: Stdout,
    current_status: Option<String>,
}

impl StdoutTerm {
    #[must_use]
    #[inline]
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
            stdout: stdout(),
            current_status: None,
        }
    }
}

impl Default for StdoutTerm {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Term for StdoutTerm {
    #[inline]
    #[expect(clippy::unwrap_used, reason = "intended to panic on error")]
    fn set_status(&mut self, status: &str) {
        let status = status.to_owned();
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

    #[inline]
    #[expect(clippy::unwrap_used, reason = "intended to panic on error")]
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

    #[inline]
    #[expect(clippy::unwrap_used, reason = "intended to panic on error")]
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
        let mut text = text.to_owned();
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
