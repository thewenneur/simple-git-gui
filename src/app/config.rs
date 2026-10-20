//! The "Git configuration" screen: each setting explained, with its git key,
//! where its value comes from, and the exact `git config` command.

use iced::widget::{column, pick_list, row};

use super::undo::Undoable;
use super::*;
use crate::git::config::{self, Kind, Origin, Scope, Setting};
use crate::git::undo::{ConfigEdit, Moves};

/// An entry of a drop-down list: the git value and its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceItem {
    value: &'static str,
    label: String,
}

impl std::fmt::Display for ChoiceItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl App {
    pub(super) fn load_config(&mut self) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let command = config::list_command(Some(&root)).background();
        Task::perform(run(command), move |record| {
            Message::ConfigLoaded(root.clone(), record)
        })
    }

    pub(super) fn config_loaded(&mut self, root: PathBuf, record: CommandRecord) -> Task<Message> {
        if let Some(repo) = self.repo.as_mut().filter(|r| r.paths.root == root) {
            if record.success() {
                repo.config = Some(config::parse(&record.stdout));
            } else {
                self.error = Some(record.error_summary());
            }
        }
        self.log(record);
        Task::none()
    }

    /// Writes a setting in the current scope (`None` removes it there).
    pub(super) fn set_config(&mut self, key: &'static str, value: Option<String>) -> Task<Message> {
        let scope = self.config_scope;
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let before = repo
            .config
            .as_ref()
            .and_then(|v| v.in_scope(key, scope))
            .map(str::to_string);
        if before == value {
            return Task::none();
        }
        self.config_drafts.remove(key);
        let command = config::set_command(&repo.paths.root, scope, key, value.as_deref());
        let label = match &value {
            Some(v) => t!("undo.config", key = key, value = v).into_owned(),
            None => t!("undo.config_unset", key = key).into_owned(),
        };
        let mut undoable = Undoable::new(label, Moves::Index);
        undoable.config = Some(ConfigEdit {
            scope,
            key: key.to_string(),
            before,
            after: value,
        });
        self.perform(Action::Config, None, Some(undoable), vec![command])
    }

    pub(super) fn apply_config_draft(&mut self, key: &'static str) -> Task<Message> {
        let Some(draft) = self.config_drafts.get(key).cloned() else {
            return Task::none();
        };
        let value = draft.trim();
        self.set_config(key, (!value.is_empty()).then(|| value.to_string()))
    }

    pub(super) fn config_view<'a>(
        &'a self,
        repo: &'a OpenRepo,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let Some(values) = &repo.config else {
            return text(t!("config.loading")).color(p.muted).into();
        };

        let mut scopes = row![].height(36);
        for (scope, key) in [
            (Scope::Local, "config.scope_local"),
            (Scope::Global, "config.scope_global"),
        ] {
            let selected = self.config_scope == scope;
            scopes = scopes.push(
                button(centered(text(tr(key)).font(sans(if selected {
                    Weight::Semibold
                } else {
                    Weight::Normal
                }))))
                .padding([0, 12])
                .height(Length::Fill)
                .on_press(Message::SetConfigScope(scope))
                .style(style::accent_segment(selected)),
            );
        }

        let header = row![
            column![
                text(t!("config.title"))
                    .size(22)
                    .font(sans(Weight::Semibold)),
                text(t!("config.intro")).color(p.text_2),
            ]
            .spacing(4)
            .width(Length::Fill),
            container(scopes).padding(1).style(style::segmented_frame),
        ]
        .spacing(16)
        .align_y(Alignment::End);

        let mut page = column![header].spacing(16).max_width(1080);
        for (section, settings) in config::SECTIONS {
            let mut card = column![
                container(
                    text(tr(&format!("config.section_{section}")))
                        .size(15)
                        .font(sans(Weight::Semibold))
                )
                .padding([12, 16]),
            ];
            for setting in *settings {
                card = card
                    .push(hline(style::separator))
                    .push(self.setting_row(setting, values, p));
            }
            page = page.push(container(card).width(Length::Fill).style(style::card));
        }

        page = page.push(self.templates_card(repo, p));

        // The command a pending text edit will run, or a hint.
        let pending = self.config_drafts.iter().next().map(|(key, draft)| {
            let value = draft.trim();
            config::set_command(
                &repo.paths.root,
                self.config_scope,
                key,
                (!value.is_empty()).then_some(value),
            )
            .display()
        });
        let footer = match pending {
            Some(command) => row![
                text("$").font(style::MONO).size(12).color(jc::PROMPT),
                text(command).font(style::MONO).size(12).width(Length::Fill),
                text(t!("config.applied_on_enter")).size(12).color(jc::HINT),
            ],
            None => row![text(t!("config.footer_hint")).size(12).color(jc::HINT)],
        };
        page = page.push(
            container(footer.spacing(10).align_y(Alignment::Center))
                .padding([10, 14])
                .width(Length::Fill)
                .style(style::journal_box),
        );

        scrollable(page.padding(Padding::default().right(12)))
            .style(style::scroll)
            .height(Length::Fill)
            .into()
    }

    fn setting_row<'a>(
        &'a self,
        setting: &'static Setting,
        values: &'a config::Values,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let key = setting.key;
        let scope = self.config_scope;
        let current = match scope {
            Scope::Local => values.effective(key).map(|(_, v)| v),
            Scope::Global => values.in_scope(key, Scope::Global),
        };
        let enabled = self.idle();

        let control: Element<'a, Message> = match setting.kind {
            Kind::Text => {
                let draft = self.config_drafts.get(key);
                let shown = draft.map(String::as_str).or(current).unwrap_or("");
                let mut input =
                    text_input(&tr(&format!("config.{}_placeholder", setting.id)), shown)
                        .size(13)
                        .padding([8, 10])
                        .style(style::input);
                if enabled {
                    input = input
                        .on_input(move |v| Message::ConfigDraft(key, v))
                        .on_submit(Message::ApplyConfig(key));
                }
                let mut line = row![input].spacing(8).align_y(Alignment::Center);
                if draft.is_some() {
                    line = line.push(
                        button(centered(text(t!("config.apply")).size(12)))
                            .padding([0, 10])
                            .height(32)
                            .on_press_maybe(enabled.then_some(Message::ApplyConfig(key)))
                            .style(style::primary),
                    );
                }
                line.into()
            }
            Kind::Bool => {
                let on = current.is_some_and(|v| matches!(v, "true" | "yes" | "on" | "1"));
                let mut check = checkbox(on)
                    .label(t!("config.enabled"))
                    .size(16)
                    .style(style::check);
                if enabled {
                    check =
                        check.on_toggle(move |on| Message::SetConfig(key, Some(on.to_string())));
                }
                check.into()
            }
            Kind::Choice(choices) => {
                let items: Vec<ChoiceItem> = choices
                    .iter()
                    .map(|(value, label)| ChoiceItem {
                        value,
                        label: tr(label),
                    })
                    .collect();
                let selected = current
                    .and_then(|c| items.iter().find(|i| i.value.eq_ignore_ascii_case(c)))
                    .cloned();
                pick_list(items, selected, move |item: ChoiceItem| {
                    Message::SetConfig(key, Some(item.value.to_string()))
                })
                .placeholder(t!("config.not_set"))
                .style(style::pick)
                .menu_style(style::menu)
                .text_size(13)
                .padding([8, 10])
                .width(Length::Fill)
                .into()
            }
        };

        // Where the value comes from, in plain words.
        let origin = match scope {
            Scope::Local => match values.effective(key).map(|(o, _)| o) {
                Some(Origin::Local) => (t!("config.origin_local"), p.warning.text),
                Some(Origin::Global) => (t!("config.origin_global"), p.muted),
                Some(Origin::System) => (t!("config.origin_system"), p.muted),
                Some(Origin::Other) => (t!("config.origin_other"), p.muted),
                None => (t!("config.not_set"), p.muted),
            },
            Scope::Global => match current {
                Some(_) => (t!("config.origin_global_scope"), p.muted),
                None => (t!("config.not_set"), p.muted),
            },
        };
        let code = match current {
            Some(v) if setting.kind != Kind::Text => format!("{key} = {v}"),
            _ => key.to_string(),
        };
        let mut info = column![
            text(code).font(style::MONO).size(12).color(p.accent.text),
            text(origin.0).size(12).color(origin.1),
        ]
        .spacing(2);
        if let Some(recommended) = setting.recommended {
            let matches = current.is_some_and(|c| c.eq_ignore_ascii_case(recommended));
            info = info.push(if matches {
                text(t!("config.recommended"))
                    .size(12)
                    .color(p.added.strong)
            } else {
                text(t!("config.advised", value = recommended))
                    .size(12)
                    .color(p.muted)
            });
        }
        let local_value = values.in_scope(key, scope).is_some();
        if scope == Scope::Local && local_value {
            info = info.push(
                button(text(t!("config.remove_local")).size(12))
                    .padding(0)
                    .on_press_maybe(enabled.then_some(Message::SetConfig(key, None)))
                    .style(style::link),
            );
        }

        container(
            row![
                column![
                    text(tr(&format!("config.{}", setting.id))).font(sans(Weight::Semibold)),
                    text(tr(&format!("config.{}_hint", setting.id)))
                        .size(13)
                        .color(p.text_2),
                ]
                .spacing(2)
                .width(Length::FillPortion(14)),
                container(control).width(Length::FillPortion(10)),
                info.width(Length::FillPortion(9)),
            ]
            .spacing(20)
            .align_y(Alignment::Center),
        )
        .padding([14, 16])
        .into()
    }
}
