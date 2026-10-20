//! The "Changes" section: the list of changed files with what goes into the
//! next commit, the commit form, and the diff of the selected file.

use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{column, mouse_area, row, text_editor};

use super::undo::Undoable;
use super::*;
use crate::git::diff::{self, DiffOptions, FileDiff, Hunk, Line, LineKind};
use crate::git::ignore::{self, IgnoreRule, IgnoreTarget};
use crate::git::ops;
use crate::git::undo::{FileChange, Moves};

/// Width of the file list card.
const LIST_WIDTH: f32 = 380.0;
/// Untracked files larger than this are not shown.
const MAX_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;
/// Lines drawn at most in the diff panel, to keep the interface fluid.
const MAX_DIFF_LINES: usize = 4000;
const LINE_HEIGHT: f32 = 22.0;

/// A file of the list: the same path can appear twice, once with its staged
/// part and once with the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub path: String,
    pub staged: bool,
}

#[derive(Debug, Clone)]
pub enum DiffKind {
    Text,
    /// Only whitespace changed and whitespace is ignored.
    WhitespaceOnly,
    Untracked,
    TooLarge,
    Conflict,
    Unreadable(String),
}

#[derive(Debug, Clone)]
pub struct DiffView {
    pub selection: Selection,
    pub options: DiffOptions,
    pub kind: DiffKind,
    pub diff: FileDiff,
    pub command: Option<String>,
    /// The hunks shown are exactly git's, so one can be staged on its own
    /// (not the case when ignoring whitespace hid some changes).
    pub hunks_applicable: bool,
}

/// What the ignore menu opened on a file is set to.
#[derive(Debug, Clone)]
pub struct IgnoreMenu {
    pub path: String,
    pub tracked: bool,
    pub untrack: bool,
}

pub async fn load_diff(
    root: PathBuf,
    selection: Selection,
    entry_kind: EntryKind,
    options: DiffOptions,
    background: bool,
) -> (DiffView, Vec<CommandRecord>) {
    let mut view = DiffView {
        selection: selection.clone(),
        options,
        kind: DiffKind::Text,
        diff: FileDiff::default(),
        command: None,
        hunks_applicable: false,
    };
    let bg = |c: GitCommand| if background { c.background() } else { c };
    match entry_kind {
        EntryKind::Conflicted => {
            view.kind = DiffKind::Conflict;
            (view, Vec::new())
        }
        EntryKind::Untracked => {
            let path = root.join(&selection.path);
            view.kind = match tokio::fs::metadata(&path).await {
                Ok(m) if m.len() > MAX_PREVIEW_BYTES => DiffKind::TooLarge,
                Ok(m) if m.is_file() => match tokio::fs::read(&path).await {
                    Ok(content) => {
                        view.diff = diff::untracked_file(&content);
                        DiffKind::Untracked
                    }
                    Err(e) => DiffKind::Unreadable(e.to_string()),
                },
                Ok(_) => DiffKind::Unreadable(t!("diff.not_a_file").into_owned()),
                Err(e) => DiffKind::Unreadable(e.to_string()),
            };
            (view, Vec::new())
        }
        EntryKind::Tracked | EntryKind::Ignored => {
            let command = diff::diff_command(&root, &selection.path, selection.staged, options);
            view.command = Some(command.display());
            let shown = run(bg(command)).await;
            let mut records = Vec::new();
            view.hunks_applicable = !options.ignore_whitespace;
            if shown.success() {
                view.diff = diff::parse(&shown.stdout, options.detect_moves);
                if options.ignore_whitespace {
                    let exact = DiffOptions {
                        ignore_whitespace: false,
                        ..options
                    };
                    let exact =
                        run(
                            diff::diff_command(&root, &selection.path, selection.staged, exact)
                                .background(),
                        )
                        .await;
                    view.hunks_applicable = exact.success() && exact.stdout == shown.stdout;
                    if view.diff.is_empty() && exact.success() && !exact.stdout.is_empty() {
                        view.kind = DiffKind::WhitespaceOnly;
                    }
                    records.push(exact);
                }
            } else {
                view.kind = DiffKind::Unreadable(shown.error_summary());
            }
            records.insert(0, shown);
            (view, records)
        }
    }
}

impl App {
    // ------------------------------------------------------------ updates

    /// Keeps the selection on an existing row after a refresh, and reloads
    /// its diff.
    pub(super) fn sync_selection(&mut self) -> Task<Message> {
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let Some(status) = &repo.status else {
            return Task::none();
        };
        let exists = |sel: &Selection| {
            status.entries.iter().any(|e| {
                e.path == sel.path
                    && if sel.staged {
                        e.has_staged()
                    } else {
                        e.has_unstaged()
                    }
            })
        };
        let previous = repo.selected.clone();
        let mut next = previous.clone().filter(exists).or_else(|| {
            previous
                .map(|s| Selection {
                    staged: !s.staged,
                    ..s
                })
                .filter(exists)
        });
        if next.is_none() {
            next = display_order(status).first().map(|(e, staged)| Selection {
                path: e.path.clone(),
                staged: *staged,
            });
        }
        let same = next == repo.selected;
        repo.selected = next;
        if !same {
            repo.diff = None;
        }
        self.load_selected_diff(same)
    }

    pub(super) fn load_selected_diff(&mut self, background: bool) -> Task<Message> {
        let options = self.diff_options;
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let (Some(selection), Some(status)) = (&repo.selected, &repo.status) else {
            return Task::none();
        };
        let Some(entry) = status.entries.iter().find(|e| e.path == selection.path) else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let task_root = root.clone();
        Task::perform(
            load_diff(root, selection.clone(), entry.kind, options, background),
            move |(view, records)| Message::DiffLoaded(task_root.clone(), Box::new(view), records),
        )
    }

    pub(super) fn diff_loaded(
        &mut self,
        root: PathBuf,
        view: DiffView,
        records: Vec<CommandRecord>,
    ) -> Task<Message> {
        for record in records {
            self.log(record);
        }
        let options = self.diff_options;
        if let Some(repo) = self.repo.as_mut().filter(|r| r.paths.root == root)
            && repo.selected.as_ref() == Some(&view.selection)
            && view.options == options
        {
            repo.diff = Some(view);
        }
        Task::none()
    }

    pub(super) fn select_file(&mut self, selection: Selection) -> Task<Message> {
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        if repo.selected.as_ref() != Some(&selection) {
            repo.selected = Some(selection);
            repo.diff = None;
        }
        repo.ignore_menu = None;
        self.load_selected_diff(false)
    }

    pub(super) fn stage_file(&mut self, path: String, stage: bool) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = &repo.paths.root;
        let label = if stage {
            t!("undo.stage", path = path).into_owned()
        } else {
            t!("undo.unstage", path = path).into_owned()
        };
        let paths = [path];
        let command = if stage {
            ops::stage(root, &paths)
        } else {
            let has_head = repo.status.as_ref().is_some_and(|s| s.branch.oid.is_some());
            ops::unstage(root, &paths, has_head)
        };
        self.perform(
            Action::Stage,
            None,
            Some(index_change(label)),
            vec![command],
        )
    }

    pub(super) fn stage_all(&mut self) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let command = ops::stage_all(&repo.paths.root);
        let label = t!("undo.stage_all").into_owned();
        self.perform(
            Action::Stage,
            None,
            Some(index_change(label)),
            vec![command],
        )
    }

    pub(super) fn apply_hunk(&mut self, index: usize) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let Some(view) = repo.diff.as_ref().filter(|v| v.hunks_applicable) else {
            return Task::none();
        };
        let Some(patch) = view.diff.hunk_patch(index) else {
            return Task::none();
        };
        let path = &view.selection.path;
        let label = if view.selection.staged {
            t!("undo.unstage_hunk", path = path).into_owned()
        } else {
            t!("undo.stage_hunk", path = path).into_owned()
        };
        let command = ops::apply_to_index(&repo.paths.root, patch, view.selection.staged);
        self.perform(
            Action::Stage,
            None,
            Some(index_change(label)),
            vec![command],
        )
    }

    pub(super) fn commit(&mut self) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let message = repo.commit_message.text().trim().to_string();
        if message.is_empty() {
            return Task::none();
        }
        let command = ops::commit(&repo.paths.root, &message);
        let summary = message.lines().next().unwrap_or_default();
        let undoable = Undoable::new(
            t!("undo.commit", message = clip_chars(summary, 48)).into_owned(),
            Moves::Commit,
        );
        self.perform(
            Action::Commit,
            Some(t!("busy.committing").into_owned()),
            Some(undoable),
            vec![command],
        )
    }

    pub(super) fn open_ignore_menu(&mut self, path: String) -> Task<Message> {
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let tracked = repo.status.as_ref().is_some_and(|s| {
            s.entries
                .iter()
                .any(|e| e.path == path && e.kind == EntryKind::Tracked)
        });
        repo.ignore_menu = Some(IgnoreMenu {
            path,
            tracked,
            untrack: tracked,
        });
        Task::none()
    }

    pub(super) fn ignore(&mut self, rule: IgnoreRule) -> Task<Message> {
        let target = self.ignore_target;
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let Some(menu) = repo.ignore_menu.take() else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let file = target.file(&root, &repo.paths.git_dir);
        let pattern = rule.pattern();
        let shell = format!(
            "echo {} >> {}",
            crate::git::runner::quote(&pattern),
            target.display()
        );
        let before = std::fs::read_to_string(&file).ok();
        let result = ignore::append(&file, &pattern)
            .map(|_| ())
            .map_err(|e| e.to_string());
        let after = std::fs::read_to_string(&file).ok();
        let ok = result.is_ok();
        let record = CommandRecord::local(shell, &root, result);
        if !ok {
            self.error = Some(record.error_summary());
        }
        self.log(record);
        let untrack = match rule {
            IgnoreRule::Folder(ref dir) => dir.clone(),
            _ => menu.path.clone(),
        };
        if !ok {
            return self.refresh(false);
        }
        let commands = if menu.tracked && menu.untrack {
            vec![ops::untrack(&root, &untrack)]
        } else {
            Vec::new()
        };
        let undoable = Undoable {
            label: t!("undo.ignore", pattern = pattern).into_owned(),
            moves: Moves::Index,
            file: Some(FileChange {
                path: file,
                before,
                after,
            }),
            config: None,
        };
        self.perform(Action::Ignore, None, Some(undoable), commands)
    }

    // -------------------------------------------------------------- views

    pub(super) fn changes_view<'a>(
        &'a self,
        repo: &'a OpenRepo,
        status: &'a Status,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        row![self.file_list(repo, status, p), self.diff_panel(repo, p)]
            .spacing(16)
            .height(Length::Fill)
            .into()
    }

    fn file_list<'a>(
        &'a self,
        repo: &'a OpenRepo,
        status: &'a Status,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let rows = display_order(status);
        let can_stage_all = self.idle()
            && rows
                .iter()
                .any(|(e, staged)| !staged && e.kind != EntryKind::Conflicted);

        let header = row![
            text(t!("changes.title"))
                .size(15)
                .font(sans(Weight::Semibold))
                .width(Length::Fill),
            button(centered(text(t!("changes.stage_all")).size(12)))
                .padding([0, 10])
                .height(28)
                .on_press_maybe(can_stage_all.then_some(Message::StageAll))
                .style(style::secondary),
        ]
        .align_y(Alignment::Center);

        let mut list = column![];
        if rows.is_empty() {
            list = list.push(container(text(t!("changes.none")).color(p.muted)).padding([16, 14]));
        }
        let mut group = None;
        for (entry, staged) in rows {
            let key = match (entry.kind, staged) {
                (EntryKind::Conflicted, _) => "changes.conflicts",
                (_, true) => "changes.staged",
                (_, false) => "changes.unstaged",
            };
            if group != Some(key) {
                group = Some(key);
                let color = match key {
                    "changes.conflicts" => p.removed.strong,
                    "changes.staged" => p.added.strong,
                    _ => p.text_2,
                };
                list = list.push(
                    container(
                        row![
                            text(tr(key).to_uppercase())
                                .size(12)
                                .font(sans(Weight::Semibold))
                                .color(color),
                            text(tr(&format!("{key}_hint")))
                                .size(11)
                                .font(style::MONO)
                                .color(p.muted),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    )
                    .padding(Padding::from([4, 14]).top(12)),
                );
            }
            list = list.push(self.file_row(repo, entry, staged, p));
        }

        container(column![
            container(header).padding([10, 14]),
            hline(style::separator),
            scrollable(list.padding(Padding::default().bottom(8)))
                .style(style::scroll)
                .height(Length::Fill),
            hline(style::separator),
            self.commit_form(repo, status, p),
        ])
        .width(LIST_WIDTH)
        .height(Length::Fill)
        .style(style::card)
        .into()
    }

    fn file_row<'a>(
        &'a self,
        repo: &'a OpenRepo,
        entry: &'a FileEntry,
        staged: bool,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let selection = Selection {
            path: entry.path.clone(),
            staged,
        };
        let selected = repo.selected.as_ref() == Some(&selection);
        let (dir, name) = split_path(&entry.path);
        let state = lowercase_first(&tr(entry.state_key()));
        let subtitle = match (&entry.orig_path, dir) {
            (Some(orig), _) => format!("{orig} → · {state}"),
            (None, Some(dir)) => format!("{dir}/ · {state}"),
            (None, None) => state,
        };

        let path = entry.path.clone();
        let mut check = checkbox(staged).size(16).style(style::check);
        if entry.kind != EntryKind::Conflicted && self.idle() {
            check = check.on_toggle(move |on| Message::StageFile(path.clone(), on));
        }

        let mut line = row![
            check,
            column![
                text(name.to_string()).size(13).font(mono(if selected {
                    Weight::Medium
                } else {
                    Weight::Normal
                })),
                text(subtitle).size(12).color(p.muted),
            ]
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        if entry.kind == EntryKind::Untracked {
            line = line.push(
                container(
                    text(t!("changes.new_badge"))
                        .size(11)
                        .font(sans(Weight::Semibold)),
                )
                .padding([1, 6])
                .style(style::badge(p.added.soft, p.added.strong)),
            );
        } else {
            // The selected file's counts come from its diff, which also
            // knows about moved lines; others come from `git diff --numstat`.
            let shown = repo
                .diff
                .as_ref()
                .filter(|d| {
                    selected && d.selection == selection && matches!(d.kind, DiffKind::Text)
                })
                .map(|d| d.diff.stats());
            let numstat = if staged {
                &repo.numstat_staged
            } else {
                &repo.numstat_unstaged
            };
            let (added, removed, moved) = match shown {
                Some(s) => (s.added, s.removed, s.moved),
                None => numstat
                    .get(&entry.path)
                    .map_or((0, 0, 0), |(a, r)| (*a, *r, 0)),
            };
            let mut counts = row![].spacing(6);
            for (value, prefix, color) in [
                (added, "+", p.added.strong),
                (removed, "−", p.removed.strong),
                (moved, "↕", p.moved.strong),
            ] {
                if value > 0 {
                    counts = counts.push(
                        text(format!("{prefix}{value}"))
                            .size(12)
                            .font(style::MONO)
                            .color(color),
                    );
                }
            }
            line = line.push(counts);
        }
        if self.advanced {
            line = line.push(
                text(entry.xy.clone())
                    .size(12)
                    .font(style::MONO)
                    .color(p.muted),
            );
        }

        let body: Element<'a, Message> = if selected {
            row![
                container(Space::new())
                    .width(3)
                    .height(Length::Fill)
                    .style(style::accent_bar),
                container(line)
                    .padding(Padding::from([0, 14]).left(11))
                    .height(Length::Fill)
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            ]
            .height(50)
            .into()
        } else {
            container(line)
                .padding([0, 14])
                .height(50)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .into()
        };
        let body = container(body).width(Length::Fill);
        let body = if selected {
            body.style(style::selected_row)
        } else {
            body
        };

        let mut area = mouse_area(body).on_press(Message::SelectFile(selection));
        if !staged && entry.kind != EntryKind::Conflicted {
            area = area.on_right_press(Message::OpenIgnoreMenu(entry.path.clone()));
        }
        area.into()
    }

    /// The dialog opened by a right-click on a file.
    pub(super) fn ignore_dialog<'a>(
        &'a self,
        menu: &'a IgnoreMenu,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let mut targets = row![].height(30);
        for (target, key) in [
            (IgnoreTarget::Shared, "ignore.shared"),
            (IgnoreTarget::Local, "ignore.local"),
        ] {
            let selected = self.ignore_target == target;
            targets = targets.push(
                button(centered(text(tr(key)).size(12)))
                    .padding([0, 10])
                    .height(Length::Fill)
                    .on_press(Message::SetIgnoreTarget(target))
                    .style(style::soft_segment(selected)),
            );
        }

        let mut menu_col = column![
            text(t!("ignore.dialog_title", path = menu.path))
                .size(20)
                .font(sans(Weight::Semibold)),
            text(t!("ignore.intro")).color(p.text_2),
            container(targets).padding(1).style(style::segmented_frame),
        ]
        .spacing(12);

        for rule in IgnoreRule::choices(&menu.path) {
            let label = match &rule {
                IgnoreRule::File(_) => t!("ignore.file").into_owned(),
                IgnoreRule::Extension(ext) => t!("ignore.extension", ext = ext).into_owned(),
                IgnoreRule::Folder(dir) => t!("ignore.folder", dir = dir).into_owned(),
            };
            let pattern = format!("{} → {}", rule.pattern(), self.ignore_target.display());
            menu_col = menu_col.push(
                button(
                    row![
                        text(label).width(Length::Fill),
                        text(pattern).size(12).font(style::MONO).color(p.muted),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([10, 12])
                .width(Length::Fill)
                .on_press_maybe(self.idle().then_some(Message::Ignore(rule)))
                .style(style::secondary),
            );
        }
        if menu.tracked {
            menu_col = menu_col.push(
                checkbox(menu.untrack)
                    .label(t!("ignore.untrack"))
                    .size(16)
                    .text_size(13)
                    .on_toggle(Message::SetUntrack)
                    .style(style::check),
            );
        }
        menu_col = menu_col.push(row![
            space::horizontal(),
            button(centered(text(t!("common.cancel"))))
                .padding([0, 16])
                .height(40)
                .on_press(Message::CloseIgnoreMenu)
                .style(style::secondary),
        ]);

        container(menu_col)
            .padding(24)
            .max_width(560)
            .style(style::dialog)
            .into()
    }

    fn commit_form<'a>(
        &'a self,
        repo: &'a OpenRepo,
        status: &'a Status,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let staged = status.entries.iter().filter(|e| e.has_staged()).count();
        let message = repo.commit_message.text();
        let message = message.trim();
        let can_commit =
            self.idle() && staged > 0 && !message.is_empty() && status.conflicts().next().is_none();
        let label = match staged {
            0 => t!("commit.button_none").into_owned(),
            1 => t!("commit.button_one").into_owned(),
            n => t!("commit.button_many", count = n).into_owned(),
        };
        let preview = ops::commit(
            &repo.paths.root,
            if message.is_empty() { "…" } else { message },
        )
        .display();

        container(
            column![
                text(t!("commit.title"))
                    .size(13)
                    .font(sans(Weight::Semibold)),
                text_editor(&repo.commit_message)
                    .placeholder(t!("commit.placeholder"))
                    .on_action(Message::CommitEdit)
                    .key_binding(|press| {
                        if super::undo::is_undo_key(&press.key, press.modifiers) {
                            None
                        } else {
                            text_editor::Binding::from_key_press(press)
                        }
                    })
                    .height(76)
                    .size(13)
                    .padding([8, 10])
                    .style(style::editor),
                button(centered(
                    container(text(label).font(sans(Weight::Semibold)))
                        .width(Length::Fill)
                        .align_x(Alignment::Center),
                ))
                .height(40)
                .width(Length::Fill)
                .on_press_maybe(can_commit.then_some(Message::Commit))
                .style(style::primary),
                text(format!("$ {preview}"))
                    .size(11)
                    .font(style::MONO)
                    .color(p.text_2),
            ]
            .spacing(8),
        )
        .padding(14)
        .width(Length::Fill)
        .style(style::card_footer)
        .into()
    }

    fn diff_panel<'a>(&'a self, repo: &'a OpenRepo, p: &'static Palette) -> Element<'a, Message> {
        let Some(selection) = &repo.selected else {
            return container(
                container(text(t!("diff.pick_file")).color(p.muted)).center(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::card)
            .into();
        };

        let options = self.diff_options;
        let mut views = row![].height(28);
        for (side, key) in [(false, "diff.unified"), (true, "diff.side_by_side")] {
            views = views.push(
                button(centered(text(tr(key)).size(12).font(sans(
                    if self.side_by_side == side {
                        Weight::Semibold
                    } else {
                        Weight::Normal
                    },
                ))))
                .padding([0, 10])
                .height(Length::Fill)
                .on_press(Message::SetSideBySide(side))
                .style(style::soft_segment(self.side_by_side == side)),
            );
        }
        let toolbar = row![
            text(selection.path.clone())
                .size(13)
                .font(mono(Weight::Medium)),
            text(if selection.staged {
                t!("diff.staged_against")
            } else {
                t!("diff.unstaged_against")
            })
            .size(12)
            .color(p.muted)
            .width(Length::Fill),
            container(views).padding(1).style(style::segmented_frame),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let mut legend = row![].spacing(14).align_y(Alignment::Center);
        for (tint, key) in [
            (p.added, "diff.legend_added"),
            (p.removed, "diff.legend_removed"),
            (p.moved, "diff.legend_moved"),
        ] {
            legend = legend.push(
                row![
                    container(Space::new())
                        .width(12)
                        .height(12)
                        .style(style::swatch(tint)),
                    text(tr(key)).size(12).color(p.text_2),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        let legend = legend
            .push(space::horizontal())
            .push(
                checkbox(options.ignore_whitespace)
                    .label(t!("diff.ignore_whitespace"))
                    .size(14)
                    .text_size(12)
                    .text_wrapping(Wrapping::None)
                    .on_toggle(Message::SetIgnoreWhitespace)
                    .style(style::check),
            )
            .push(
                checkbox(options.detect_moves)
                    .label(t!("diff.show_moves"))
                    .size(14)
                    .text_size(12)
                    .text_wrapping(Wrapping::None)
                    .on_toggle(Message::SetShowMoves)
                    .style(style::check),
            );

        let body: Element<'a, Message> = match &repo.diff {
            None => centered_note(t!("diff.loading").into_owned(), p),
            Some(view) => match &view.kind {
                DiffKind::Conflict => centered_note(t!("diff.conflict").into_owned(), p),
                DiffKind::TooLarge => centered_note(t!("diff.too_large").into_owned(), p),
                DiffKind::WhitespaceOnly => {
                    centered_note(t!("diff.whitespace_only").into_owned(), p)
                }
                DiffKind::Unreadable(e) => centered_note(e.clone(), p),
                _ if view.diff.binary => centered_note(t!("diff.binary").into_owned(), p),
                _ if view.diff.is_empty() => centered_note(t!("diff.empty").into_owned(), p),
                kind => scrollable(self.hunks(view, matches!(kind, DiffKind::Text), p))
                    .style(style::scroll)
                    .height(Length::Fill)
                    .into(),
            },
        };

        let footer = match repo.diff.as_ref() {
            Some(DiffView {
                command: Some(command),
                ..
            }) => format!("$ {command}"),
            Some(DiffView {
                kind: DiffKind::Untracked,
                ..
            }) => t!("diff.untracked_footer").into_owned(),
            _ => String::new(),
        };

        container(column![
            container(toolbar).padding([10, 14]),
            hline(style::separator),
            container(legend).padding([8, 14]),
            hline(style::separator),
            container(body).height(Length::Fill),
            hline(style::separator),
            container(text(footer).size(11).font(style::MONO).color(p.text_2)).padding([8, 14]),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::card)
        .into()
    }

    fn hunks<'a>(
        &'a self,
        view: &'a DiffView,
        tracked: bool,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let mut col = column![];
        let mut budget = MAX_DIFF_LINES;
        for (index, hunk) in view.diff.hunks.iter().enumerate() {
            col = col.push(self.hunk_header(view, index, hunk, tracked, p));
            let shown = hunk.lines.len().min(budget);
            if self.side_by_side {
                col = col.push(side_by_side(&hunk.lines[..shown], p));
            } else {
                col = col.push(unified(&hunk.lines[..shown], p));
            }
            budget -= shown;
            if budget == 0 {
                let total: usize = view.diff.hunks.iter().map(|h| h.lines.len()).sum();
                col = col.push(
                    container(
                        text(t!("diff.truncated", count = total - MAX_DIFF_LINES))
                            .size(12)
                            .color(p.muted),
                    )
                    .padding([8, 14]),
                );
                break;
            }
        }
        col.into()
    }

    fn hunk_header<'a>(
        &'a self,
        view: &'a DiffView,
        index: usize,
        hunk: &'a Hunk,
        tracked: bool,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let (from, to) = hunk_range(hunk);
        let only_moved_out = hunk
            .lines
            .iter()
            .filter(|l| l.kind != LineKind::Context)
            .all(|l| l.kind == LineKind::Removed && l.is_moved());
        let mut title = row![].align_y(Alignment::Center);
        if only_moved_out {
            title = title.push(text(t!("diff.old_place", from = from, to = to)).size(12));
        } else if hunk.section.is_empty() {
            title = title.push(text(t!("diff.lines", from = from, to = to)).size(12));
        } else {
            title = title
                .push(text(t!("diff.in_before")).size(12))
                .push(
                    text(hunk.section.clone())
                        .size(12)
                        .font(mono(Weight::Medium))
                        .color(p.text),
                )
                .push(text(t!("diff.in_after", from = from, to = to)).size(12));
        }
        let mut header = row![container(title).width(Length::Fill).clip(true),]
            .spacing(10)
            .align_y(Alignment::Center);

        if tracked {
            let label = if view.selection.staged {
                t!("diff.unstage_hunk")
            } else {
                t!("diff.stage_hunk")
            };
            let enabled = view.hunks_applicable && self.idle();
            let hunk_button = button(centered(text(label).size(12)))
                .padding([0, 10])
                .height(26)
                .on_press_maybe(enabled.then_some(Message::ApplyHunk(index)))
                .style(style::secondary);
            if view.hunks_applicable {
                header = header.push(hunk_button);
            } else {
                header = header.push(tooltip(
                    hunk_button,
                    container(text(t!("diff.hunk_needs_exact")).size(12))
                        .padding([4, 8])
                        .max_width(280)
                        .style(style::card),
                    tooltip::Position::Left,
                ));
            }
        }
        container(header)
            .padding([6, 14])
            .width(Length::Fill)
            .style(style::hunk_header)
            .into()
    }
}

fn index_change(label: String) -> Undoable {
    Undoable::new(label, Moves::Index)
}

/// Shortens `s` to `max` characters, with an ellipsis.
fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

/// Rows of the list in display order: conflicts, staged, then not staged.
/// The bool tells whether the row stands for the staged part of the file.
fn display_order(status: &Status) -> Vec<(&FileEntry, bool)> {
    let conflicts = status.conflicts().map(|e| (e, false));
    let staged = status
        .entries
        .iter()
        .filter(|e| e.has_staged())
        .map(|e| (e, true));
    let unstaged = status
        .entries
        .iter()
        .filter(|e| matches!(e.kind, EntryKind::Tracked | EntryKind::Untracked) && e.has_unstaged())
        .map(|e| (e, false));
    conflicts.chain(staged).chain(unstaged).collect()
}

/// First and last line of a hunk, on the new side unless it only removes.
fn hunk_range(hunk: &Hunk) -> (u32, u32) {
    let (start, len) = if hunk.new_len > 0 {
        (hunk.new_start, hunk.new_len)
    } else {
        (hunk.old_start, hunk.old_len)
    };
    (start, start + len.saturating_sub(1))
}

fn centered_note<'a>(note: String, p: &'static Palette) -> Element<'a, Message> {
    container(text(note).color(p.muted))
        .center(Length::Fill)
        .padding(20)
        .into()
}

/// Background, text color, number color and sign of a diff line.
fn line_look(line: &Line, p: &'static Palette) -> (Color, Color, Color, &'static str) {
    match (line.kind, line.is_moved()) {
        (LineKind::Context, _) => (Color::TRANSPARENT, p.text, p.line_number, ""),
        (_, true) => (p.moved.soft, p.moved.text, p.moved.strong, "↕"),
        (LineKind::Added, false) => (p.added.soft, p.added.text, p.added.strong, "+"),
        (LineKind::Removed, false) => (p.removed.soft, p.removed.text, p.removed.strong, "−"),
    }
}

fn number<'a>(n: Option<u32>, color: Color) -> Element<'a, Message> {
    container(
        text(n.map(|n| n.to_string()).unwrap_or_default())
            .size(13)
            .font(style::MONO)
            .line_height(LineHeight::Absolute(LINE_HEIGHT.into()))
            .color(color),
    )
    .width(44)
    .padding(Padding::default().right(8))
    .align_x(Alignment::End)
    .into()
}

fn code<'a>(line: &Line, color: Color) -> Element<'a, Message> {
    text(line.text.replace('\t', "    "))
        .size(13)
        .font(style::MONO)
        .line_height(LineHeight::Absolute(LINE_HEIGHT.into()))
        .color(color)
        .width(Length::Fill)
        .into()
}

fn sign<'a>(s: &'static str, color: Color) -> Element<'a, Message> {
    container(
        text(s)
            .size(13)
            .font(style::MONO)
            .line_height(LineHeight::Absolute(LINE_HEIGHT.into()))
            .color(color),
    )
    .width(20)
    .into()
}

/// The note under a moved block, saying where it comes from or goes to.
fn moved_note<'a>(line: &Line, indent: f32, p: &'static Palette) -> Element<'a, Message> {
    let note = match (line.moved_from, line.moved_to) {
        (Some(from), _) => t!("diff.moved_from", line = from).into_owned(),
        (_, Some(to)) => t!("diff.moved_to", line = to).into_owned(),
        _ => String::new(),
    };
    container(text(note).size(12).color(p.moved.strong))
        .padding(Padding::default().left(indent).bottom(4))
        .width(Length::Fill)
        .style(style::line(p.moved.soft))
        .into()
}

/// Whether `line` ends a run of moved lines (to put the note after it).
fn ends_moved_run(line: &Line, next: Option<&Line>) -> bool {
    line.is_moved() && next.is_none_or(|n| !n.is_moved() || n.kind != line.kind)
}

fn unified<'a>(lines: &'a [Line], p: &'static Palette) -> Element<'a, Message> {
    let mut col = column![];
    for (i, line) in lines.iter().enumerate() {
        let (background, color, number_color, mark) = line_look(line, p);
        let old_color = if line.kind == LineKind::Added {
            p.line_number
        } else {
            number_color
        };
        let new_color = if line.kind == LineKind::Removed {
            p.line_number
        } else {
            number_color
        };
        col = col.push(
            container(row![
                number(line.old, old_color),
                number(line.new, new_color),
                sign(mark, number_color),
                code(line, color),
            ])
            .width(Length::Fill)
            .style(style::line(background)),
        );
        if ends_moved_run(line, lines.get(i + 1)) {
            col = col.push(moved_note(line, 108.0, p));
        }
    }
    col.into()
}

/// Old version on the left, new on the right; removed and added lines that
/// follow each other are shown face to face.
fn side_by_side<'a>(lines: &'a [Line], p: &'static Palette) -> Element<'a, Message> {
    let mut pairs: Vec<(Option<&Line>, Option<&Line>)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind == LineKind::Context {
            pairs.push((Some(&lines[i]), Some(&lines[i])));
            i += 1;
            continue;
        }
        let removed: Vec<&Line> = lines[i..]
            .iter()
            .take_while(|l| l.kind == LineKind::Removed)
            .collect();
        i += removed.len();
        let added: Vec<&Line> = lines[i..]
            .iter()
            .take_while(|l| l.kind == LineKind::Added)
            .collect();
        i += added.len();
        for k in 0..removed.len().max(added.len()) {
            pairs.push((removed.get(k).copied(), added.get(k).copied()));
        }
    }

    let half = |line: Option<&Line>, old: bool| -> Element<'a, Message> {
        match line {
            None => container(Space::new())
                .width(Length::FillPortion(1))
                .height(Length::Fill)
                .style(style::line(p.page))
                .into(),
            Some(line) => {
                let (background, color, number_color, mark) = line_look(line, p);
                container(row![
                    number(if old { line.old } else { line.new }, number_color),
                    sign(mark, number_color),
                    code(line, color),
                ])
                .width(Length::FillPortion(1))
                .style(style::line(background))
                .into()
            }
        }
    };

    let mut col = column![];
    for (left, right) in pairs {
        col = col.push(row![half(left, true), vline(), half(right, false)].height(Length::Shrink));
    }
    col.into()
}
