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

pub struct Term {
    stdout: Stdout,
    current_status: Option<String>,
}

pub fn term() -> ArcMutexGuard<RawMutex, Term> {
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
    term().debug(text)
}

pub fn info(text: impl Display) {
    term().info(text)
}

pub fn warn(text: impl Display) {
    term().warn(text)
}

pub fn error(text: impl Display) {
    term().error(text)
}

impl Term {
    pub fn new() -> Self {
        Self {
            stdout: std::io::stdout(),
            current_status: None,
        }
    }

    pub fn set_status(&mut self, status: impl Display) {
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
        self.stdout.write_all(status.as_bytes()).unwrap();
        self.stdout.queue(cursor::RestorePosition).unwrap();
        self.stdout.flush().unwrap();
        self.current_status = Some(status);
    }

    pub fn clear_status(&mut self) {
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

    fn write(&mut self, color: Color, text: impl Display) {
        let old_status = self.current_status.clone();
        self.clear_status();
        self.stdout.queue(SetForegroundColor(color)).unwrap();
        self.stdout
            .write_all(format!("{text}\n").as_bytes())
            .unwrap();
        self.stdout.queue(ResetColor).unwrap();
        if let Some(old_status) = old_status {
            self.set_status(old_status);
        }
        self.stdout.flush().unwrap();
    }

    pub fn debug(&mut self, text: impl Display) {
        self.write(Color::Grey, text)
    }

    pub fn info(&mut self, text: impl Display) {
        self.write(Color::Green, text)
    }

    pub fn warn(&mut self, text: impl Display) {
        self.write(Color::DarkYellow, text)
    }

    pub fn error(&mut self, text: impl Display) {
        self.write(Color::Red, text)
    }
}

impl Default for Term {
    fn default() -> Self {
        Self::new()
    }
}
