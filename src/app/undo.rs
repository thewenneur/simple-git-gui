//! Ctrl+Z / Ctrl+Y: every action that changes the repository records where
//! HEAD and the index were before and after, so it can be undone and redone.

use std::future::Future;

use iced::event::{self, Event};
use iced::keyboard::{self, Key};
use iced::widget::row;
use iced::window;

use super::*;
use crate::git::undo::{self, ConfigEdit, Entry, FileChange, Moves, Refusal};

/// The most actions kept for undo.
const UNDO_LIMIT: usize = 50;

/// What an action needs to become undoable.
pub struct Undoable {
    pub label: String,
    pub moves: Moves,
    pub file: Option<FileChange>,
    pub config: Option<ConfigEdit>,
}

impl Undoable {
    pub fn new(label: String, moves: Moves) -> Self {
        Self {
            label,
            moves,
            file: None,
            config: None,
        }
    }
}

/// The undo and redo stacks of an open repository.
#[derive(Default)]
pub struct UndoStacks {
    pub undo: Vec<Entry>,
    pub redo: Vec<Entry>,
    /// Set after a push: earlier actions are on the server and can't be
    /// undone locally. Holds what was sent.
    pub after_push: Option<String>,
}

impl UndoStacks {
    fn record(&mut self, entry: Entry) {
        self.undo.push(entry);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.after_push = None;
    }

    pub fn clear_after_push(&mut self, sent: String) {
        self.undo.clear();
        self.redo.clear();
        self.after_push = Some(sent);
    }
}

/// Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z, when no widget used the key.
pub fn shortcut(event: Event, status: event::Status, _window: window::Id) -> Option<Message> {
    if status == event::Status::Captured {
        return None;
    }
    let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    if !modifiers.command() {
        return None;
    }
    match key.as_ref() {
        Key::Character(c) if c.eq_ignore_ascii_case("z") && modifiers.shift() => {
            Some(Message::Redo)
        }
        Key::Character(c) if c.eq_ignore_ascii_case("z") => Some(Message::Undo),
        Key::Character(c) if c.eq_ignore_ascii_case("y") => Some(Message::Redo),
        _ => None,
    }
}

/// Whether a key press is an undo/redo shortcut (so text fields leave it to
/// the app).
pub fn is_undo_key(key: &Key, modifiers: keyboard::Modifiers) -> bool {
    modifiers.command()
        && matches!(key.as_ref(), Key::Character(c) if c.eq_ignore_ascii_case("z") || c.eq_ignore_ascii_case("y"))
}

impl App {
    /// Runs `work` as a user action. When `undoable` is given, the state is
    /// recorded before and after so the action can be undone.
    pub(super) fn perform_with(
        &mut self,
        action: Action,
        label: Option<String>,
        undoable: Option<Undoable>,
        work: impl Future<Output = Vec<CommandRecord>> + Send + 'static,
    ) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        self.busy = Some(label.unwrap_or_default());
        self.notice = None;
        Task::perform(
            async move {
                let Some(undoable) = undoable else {
                    return Message::ActionDone(action, work.await, None);
                };
                let (before, mut records) = undo::snapshot(&root).await;
                records.extend(work.await);
                let (after, after_records) = undo::snapshot(&root).await;
                let entry = Entry {
                    label: undoable.label,
                    moves: undoable.moves,
                    before,
                    after,
                    file: undoable.file,
                    config: undoable.config,
                };
                // Keep the action's own result last: it decides success.
                let last = records.pop();
                records.extend(after_records);
                records.extend(last);
                Message::ActionDone(action, records, Some(Box::new(entry)))
            },
            |message| message,
        )
    }

    /// Records a finished action for undo, if it changed something.
    pub(super) fn record_undo(&mut self, entry: Entry) {
        if let Some(repo) = &mut self.repo
            && entry.changed_something()
        {
            repo.undo.record(entry);
        }
    }

    pub(super) fn undo_or_redo(&mut self, undoing: bool) -> Task<Message> {
        if !self.idle() || self.send.is_some() {
            return Task::none();
        }
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let stack = if undoing {
            &repo.undo.undo
        } else {
            &repo.undo.redo
        };
        let Some(entry) = stack.last() else {
            return Task::none();
        };
        let step = entry.step(undoing);
        self.busy = Some(if undoing {
            t!("busy.undoing", action = entry.label).into_owned()
        } else {
            t!("busy.redoing", action = entry.label).into_owned()
        });
        self.notice = None;
        let root = repo.paths.root.clone();
        Task::perform(undo::apply(root, step), move |(result, records)| {
            Message::UndoDone(undoing, result, records)
        })
    }

    pub(super) fn undo_done(
        &mut self,
        undoing: bool,
        result: Result<(), Refusal>,
        records: Vec<CommandRecord>,
    ) -> Task<Message> {
        self.busy = None;
        for record in records {
            self.log(record);
        }
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let stacks = &mut repo.undo;
        match result {
            Ok(()) => {
                let (from, to) = if undoing {
                    (&mut stacks.undo, &mut stacks.redo)
                } else {
                    (&mut stacks.redo, &mut stacks.undo)
                };
                if let Some(entry) = from.pop() {
                    self.notice = Some(if undoing {
                        t!("undo.done", action = entry.label).into_owned()
                    } else {
                        t!("undo.redone", action = entry.label).into_owned()
                    });
                    to.push(entry);
                }
            }
            Err(Refusal::ChangedMeanwhile) => {
                // The stacks describe a past that no longer exists.
                stacks.undo.clear();
                stacks.redo.clear();
                self.error = Some(t!("error.undo_changed").into_owned());
            }
            Err(Refusal::Failed(detail)) => {
                self.error = Some(t!("error.undo_failed", detail = detail).into_owned());
            }
        }
        let refresh = self.refresh(false);
        if self
            .repo
            .as_ref()
            .is_some_and(|r| r.section == Section::Config)
        {
            Task::batch([refresh, self.load_config()])
        } else {
            refresh
        }
    }

    pub(super) fn undo_buttons<'a>(
        &'a self,
        repo: &'a OpenRepo,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let stacks = &repo.undo;
        let ready = self.idle() && self.send.is_none();
        let undo_tip = match (stacks.undo.last(), &stacks.after_push) {
            (Some(e), _) => t!("undo.tooltip", action = e.label).into_owned(),
            (None, Some(sent)) => t!("undo.after_push", action = sent).into_owned(),
            (None, None) => t!("undo.nothing").into_owned(),
        };
        let redo_tip = match stacks.redo.last() {
            Some(e) => t!("undo.redo_tooltip", action = e.label).into_owned(),
            None => t!("undo.nothing_to_redo").into_owned(),
        };
        let button_for = |glyph: Icon, enabled: bool, message: Message, tip: String| {
            let color = if enabled { p.text } else { p.disabled };
            tooltip(
                button(container(icon(glyph, 16.0, color)).center(Length::Fill))
                    .width(36)
                    .height(36)
                    .padding(0)
                    .on_press_maybe(enabled.then_some(message))
                    .style(style::secondary),
                container(text(tip).size(12))
                    .padding([4, 8])
                    .max_width(320)
                    .style(style::card),
                tooltip::Position::Bottom,
            )
        };
        row![
            button_for(
                Icon::Undo,
                ready && !stacks.undo.is_empty(),
                Message::Undo,
                undo_tip
            ),
            button_for(
                Icon::Redo,
                ready && !stacks.redo.is_empty(),
                Message::Redo,
                redo_tip
            ),
        ]
        .spacing(4)
        .into()
    }
}
