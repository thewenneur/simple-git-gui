//! A commit message typed in two parts, as Git expects it: a short summary
//! (the header shown in lists) and an optional description.

use iced::widget::{column, row, text, text_editor, text_input};
use iced::{Element, Length, Task};

use super::*;

/// Past this, lists start shortening the summary.
pub const SUMMARY_SOFT: usize = 50;
/// Past this, most Git tools cut the summary.
pub const SUMMARY_HARD: usize = 72;
/// The usual width of description lines.
pub const BODY_WIDTH: usize = 72;

#[derive(Debug, Default)]
pub struct Draft {
    pub summary: String,
    pub body: text_editor::Content,
}

impl Clone for Draft {
    fn clone(&self) -> Self {
        Self {
            summary: self.summary.clone(),
            body: text_editor::Content::with_text(&self.body.text()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Edit {
    Summary(String),
    /// Enter in the summary: go on in the description.
    Next,
    Body(text_editor::Action),
}

impl Draft {
    /// Splits an existing message into its summary and description.
    pub fn from_message(message: &str) -> Self {
        let (summary, description) = split(message);
        Self {
            summary,
            body: text_editor::Content::with_text(&description),
        }
    }

    pub fn summary(&self) -> &str {
        self.summary.trim()
    }

    pub fn description(&self) -> String {
        tidy(&self.body.text())
    }

    /// The whole message, as Git stores it.
    pub fn message(&self) -> String {
        let description = self.description();
        if description.is_empty() {
            self.summary().to_string()
        } else {
            format!("{}\n\n{description}", self.summary())
        }
    }

    /// A commit needs a summary; the description is optional.
    pub fn is_empty(&self) -> bool {
        self.summary().is_empty()
    }

    /// Applies an edit; `body_id` is the description's widget, focused on Enter.
    pub fn perform<M: Send + 'static>(&mut self, edit: Edit, body_id: &'static str) -> Task<M> {
        match edit {
            Edit::Summary(value) => {
                // A pasted multi-line message: its first line is the summary,
                // the rest goes in front of the description.
                if value.contains('\n') {
                    let (summary, rest) = split(&value);
                    let body = self.description();
                    let body = [rest, body]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    self.summary = summary;
                    self.body = text_editor::Content::with_text(&body);
                } else {
                    self.summary = value;
                }
                Task::none()
            }
            Edit::Next => iced::widget::operation::focus(body_id),
            Edit::Body(action) => {
                self.body.perform(action);
                Task::none()
            }
        }
    }

    /// The summary box with its length hint, then the description box.
    pub fn view<'a>(
        &'a self,
        on_edit: fn(Edit) -> Message,
        body_id: &'static str,
        body_height: f32,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let length = self.summary().chars().count();
        let counter = text(format!("{length}/{SUMMARY_SOFT}"))
            .size(11)
            .font(style::MONO)
            .color(if length > SUMMARY_HARD {
                p.removed.text
            } else if length > SUMMARY_SOFT {
                p.warning.text
            } else {
                p.muted
            });
        let summary = row![
            text_input(&t!("commit.summary_placeholder"), &self.summary)
                .size(13)
                .padding([8, 10])
                .width(Length::Fill)
                .on_input(move |value| on_edit(Edit::Summary(value)))
                .on_submit(on_edit(Edit::Next))
                .style(style::input),
            counter,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let hint = if length > SUMMARY_HARD {
            Some(text(t!("commit.summary_too_long")).color(p.removed.text))
        } else if length > SUMMARY_SOFT {
            Some(text(t!("commit.summary_long")).color(p.warning.text))
        } else if self
            .body
            .text()
            .lines()
            .any(|line| line.chars().count() > BODY_WIDTH)
        {
            Some(text(t!("commit.body_long_lines")).color(p.muted))
        } else {
            None
        };

        column![
            summary,
            text_editor(&self.body)
                .id(body_id)
                .placeholder(t!("commit.description_placeholder"))
                .on_action(move |action| on_edit(Edit::Body(action)))
                .key_binding(|press| {
                    if super::undo::is_undo_key(&press.key, press.modifiers) {
                        None
                    } else {
                        text_editor::Binding::from_key_press(press)
                    }
                })
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                .height(body_height)
                .size(13)
                .padding([8, 10])
                .style(style::editor),
        ]
        .push(hint.map(|hint| hint.size(12)))
        .spacing(6)
        .into()
    }
}

/// The first non-blank line, and the rest without its surrounding blank lines.
pub fn split(message: &str) -> (String, String) {
    let message = message.trim_start();
    match message.split_once('\n') {
        Some((summary, rest)) => (summary.trim().to_string(), tidy(rest)),
        None => (message.trim().to_string(), String::new()),
    }
}

/// Drops blank lines at both ends and trailing spaces on each line.
fn tidy(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let start = lines.iter().position(|l| !l.is_empty());
    let end = lines.iter().rposition(|l| !l.is_empty());
    match (start, end) {
        (Some(start), Some(end)) => lines[start..=end].join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_summary_and_description() {
        assert_eq!(split("Fix it"), ("Fix it".into(), String::new()));
        assert_eq!(
            split("\n  Fix it  \n\n\nWhy:\n  because  \n\n"),
            ("Fix it".into(), "Why:\n  because".into())
        );
    }

    #[test]
    fn joins_with_a_blank_line() {
        let mut draft = Draft::from_message("Title\n\nBody line\n");
        assert_eq!(draft.message(), "Title\n\nBody line");
        draft.body = text_editor::Content::new();
        assert_eq!(draft.message(), "Title");
        draft.summary = "  ".into();
        assert!(draft.is_empty());
    }

    #[test]
    fn pasting_lines_in_the_summary_moves_them_down() {
        let mut draft = Draft::from_message("Old\n\nkept");
        let _: Task<()> = draft.perform(Edit::Summary("New title\nfirst detail".into()), "x");
        assert_eq!(draft.summary(), "New title");
        assert_eq!(draft.description(), "first detail\n\nkept");
    }
}
