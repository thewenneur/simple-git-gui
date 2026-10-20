//! The "History and branches" section: the commit graph, the details of a
//! commit (or of a comparison), the right-click menus of commits and
//! branches, and the dialogs they open.

use iced::widget::canvas::{self, Canvas, Frame, Path as Shape, Stroke};
use iced::widget::{column, mouse_area, row, text_editor};
use iced::{Point, Rectangle, Renderer, mouse};

use super::changes::{hunk_range, side_by_side, unified};
use super::undo::Undoable;
use super::*;
use crate::git::branches::{self, ChangedFile, Comparison, Target};
use crate::git::diff::{self, FileDiff};
use crate::git::graph::{self, Half};
use crate::git::undo::Moves;

pub const ROW_HEIGHT: f32 = 40.0;
const LANE_WIDTH: f32 = 22.0;
const LANE_OFFSET: f32 = 16.0;
const DETAILS_WIDTH: f32 = 470.0;
const MENU_WIDTH: f32 = 380.0;
const MENU_ITEM_HEIGHT: f32 = 38.0;
/// Lines of a commit's diff drawn at most.
const MAX_DIFF_LINES: usize = 3000;

/// What the history section shows, per open repository.
#[derive(Default)]
pub struct HistoryState {
    pub rows: Vec<graph::Row>,
    /// The commit clicked last.
    pub selected: Option<String>,
    /// A second commit picked with Ctrl+click, to compare with `selected`.
    pub compare: Option<String>,
    pub details: Option<Details>,
    pub search: String,
    /// A commit or label pressed, and maybe being dragged.
    pub drag: Option<super::dragdrop::Drag>,
    /// What is under the pointer, where a drag would drop.
    pub hover: Option<super::dragdrop::Grip>,
}

/// What a commit (or a comparison) changed.
#[derive(Debug, Clone)]
pub struct Details {
    pub comparison: Comparison,
    /// The full message of the commit (empty for comparisons).
    pub message: String,
    pub files: Vec<ChangedFile>,
    pub file: Option<String>,
    pub diff: Option<FileDiff>,
    pub command: Option<String>,
}

/// What a right-click was made on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    Commit(String),
    /// A branch, local (`main`) or remote (`origin/main`).
    Branch(String),
}

/// A right-click menu, drawn where the click happened.
#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub at: Point,
    pub target: MenuTarget,
}

/// The actions offered in the menus.
#[derive(Debug, Clone)]
pub enum MenuAction {
    NewBranch(String),
    NewTag(String),
    ShowCommit(String),
    CompareWithFiles(String),
    /// The project's files as they were at this commit.
    Browse(String),
    CherryPick(String),
    Revert(String),
    Reword(String),
    CopyId(String),
    Switch(String),
    Merge(String),
    Rebase(String),
    DeleteBranch(String),
}

/// A dialog opened from a menu, for actions that need a name or a message.
#[derive(Debug)]
pub enum Dialog {
    NewBranch {
        at: String,
        name: String,
        switch: bool,
    },
    NewTag {
        at: String,
        name: String,
    },
    Reword {
        commit: String,
        content: text_editor::Content,
    },
    /// A drag and drop to confirm (`squash`: the choice for two neighbours).
    Drop {
        plan: super::dragdrop::DropPlan,
        squash: bool,
    },
    /// `git branch -d` refused: the branch has commits found nowhere else.
    ForceDelete {
        branch: String,
    },
}

impl Clone for Dialog {
    fn clone(&self) -> Self {
        match self {
            Dialog::NewBranch { at, name, switch } => Dialog::NewBranch {
                at: at.clone(),
                name: name.clone(),
                switch: *switch,
            },
            Dialog::NewTag { at, name } => Dialog::NewTag {
                at: at.clone(),
                name: name.clone(),
            },
            Dialog::Reword { commit, content } => Dialog::Reword {
                commit: commit.clone(),
                content: text_editor::Content::with_text(&content.text()),
            },
            Dialog::Drop { plan, squash } => Dialog::Drop {
                plan: plan.clone(),
                squash: *squash,
            },
            Dialog::ForceDelete { branch } => Dialog::ForceDelete {
                branch: branch.clone(),
            },
        }
    }
}

async fn load_details(
    root: PathBuf,
    comparison: Comparison,
    ignore_whitespace: bool,
) -> (Details, Vec<CommandRecord>) {
    let files = run(branches::changed_files_command(&root, &comparison)).await;
    let mut records = Vec::new();
    let message = match &comparison {
        Comparison::Commit { hash, .. } => {
            let show = run(
                GitCommand::new(["show", "-s", "--format=%B", hash.as_str()])
                    .in_dir(&root)
                    .background(),
            )
            .await;
            let message = show.stdout.trim().to_string();
            records.push(show);
            message
        }
        _ => String::new(),
    };
    let list = branches::parse_changed_files(&files.stdout);
    records.insert(0, files);
    let mut details = Details {
        comparison,
        message,
        files: list,
        file: None,
        diff: None,
        command: None,
    };
    if let Some(first) = details.files.first().map(|f| f.path.clone()) {
        let (diff, command, record) =
            load_file_diff(&root, &details.comparison, &first, ignore_whitespace).await;
        details.file = Some(first);
        details.diff = Some(diff);
        details.command = Some(command);
        records.push(record);
    }
    (details, records)
}

async fn load_file_diff(
    root: &Path,
    comparison: &Comparison,
    path: &str,
    ignore_whitespace: bool,
) -> (FileDiff, String, CommandRecord) {
    let command = branches::file_diff_command(root, comparison, path, ignore_whitespace);
    let shown = command.display();
    let record = run(command).await;
    (diff::parse(&record.stdout, true), shown, record)
}

impl App {
    // ------------------------------------------------------------ updates

    /// Recomputes the graph after the history was re-read.
    pub(super) fn history_refreshed(&mut self) {
        if let Some(repo) = &mut self.repo {
            repo.history.rows = graph::layout(&repo.commits);
            let known = |h: &Option<String>| {
                h.as_ref()
                    .is_none_or(|h| repo.commits.iter().any(|c| c.hash == *h))
            };
            if !known(&repo.history.selected) || !known(&repo.history.compare) {
                repo.history.selected = None;
                repo.history.compare = None;
                repo.history.details = None;
            }
        }
    }

    /// A click on a commit: shows what it changed. With Ctrl, compares it
    /// with the commit selected before.
    pub(super) fn select_commit(&mut self, hash: String) -> Task<Message> {
        let ctrl = self.modifiers.command();
        let ignore_whitespace = self.diff_options.ignore_whitespace;
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let history = &mut repo.history;
        let comparison = match (&history.selected, ctrl) {
            (Some(first), true) if *first != hash => {
                history.compare = Some(hash.clone());
                // Older commit first, so the diff reads forward.
                let position = |h: &str| repo.commits.iter().position(|c| c.hash == h);
                let (from, to) = if position(first) > position(&hash) {
                    (first.clone(), hash.clone())
                } else {
                    (hash.clone(), first.clone())
                };
                Comparison::Between { from, to }
            }
            _ => {
                history.selected = Some(hash.clone());
                history.compare = None;
                let parent = repo
                    .commits
                    .iter()
                    .find(|c| c.hash == hash)
                    .and_then(|c| c.parents.first().cloned());
                Comparison::Commit { hash, parent }
            }
        };
        self.show_comparison(comparison, ignore_whitespace)
    }

    fn show_comparison(&mut self, comparison: Comparison, ignore_ws: bool) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        Task::perform(
            load_details(root.clone(), comparison, ignore_ws),
            move |(details, records)| {
                Message::CommitDetails(root.clone(), Box::new(details), records)
            },
        )
    }

    pub(super) fn details_loaded(
        &mut self,
        root: PathBuf,
        details: Details,
        records: Vec<CommandRecord>,
    ) -> Task<Message> {
        for record in records {
            self.log(record);
        }
        if let Some(repo) = self.repo.as_mut().filter(|r| r.paths.root == root) {
            repo.history.details = Some(details);
        }
        Task::none()
    }

    pub(super) fn select_commit_file(&mut self, path: String) -> Task<Message> {
        let ignore_whitespace = self.diff_options.ignore_whitespace;
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let Some(details) = &mut repo.history.details else {
            return Task::none();
        };
        details.file = Some(path.clone());
        let root = repo.paths.root.clone();
        let comparison = details.comparison.clone();
        Task::perform(
            async move {
                let (diff, command, record) =
                    load_file_diff(&root, &comparison, &path, ignore_whitespace).await;
                Message::CommitFileDiff(root, path, Box::new(diff), command, record)
            },
            |message| message,
        )
    }

    pub(super) fn commit_file_diff(
        &mut self,
        root: PathBuf,
        path: String,
        diff: FileDiff,
        command: String,
        record: CommandRecord,
    ) -> Task<Message> {
        self.log(record);
        if let Some(details) = self
            .repo
            .as_mut()
            .filter(|r| r.paths.root == root)
            .and_then(|r| r.history.details.as_mut())
            .filter(|d| d.file.as_deref() == Some(path.as_str()))
        {
            details.diff = Some(diff);
            details.command = Some(command);
        }
        Task::none()
    }

    pub(super) fn open_menu(&mut self, target: MenuTarget) -> Task<Message> {
        if !self.idle() {
            return Task::none();
        }
        self.context_menu = Some(ContextMenu {
            at: self.pointer,
            target,
        });
        Task::none()
    }

    pub(super) fn menu_action(&mut self, action: MenuAction) -> Task<Message> {
        self.context_menu = None;
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let is_merge = |hash: &str| {
            repo.commits
                .iter()
                .find(|c| c.hash == hash)
                .is_some_and(|c| c.parents.len() > 1)
        };
        let short = |hash: &str| hash.chars().take(7).collect::<String>();
        match action {
            MenuAction::NewBranch(at) => {
                self.history_dialog = Some(Dialog::NewBranch {
                    at,
                    name: String::new(),
                    switch: true,
                });
                Task::none()
            }
            MenuAction::NewTag(at) => {
                self.history_dialog = Some(Dialog::NewTag {
                    at,
                    name: String::new(),
                });
                Task::none()
            }
            MenuAction::ShowCommit(hash) => self.select_commit(hash),
            MenuAction::Browse(hash) => self.viewer_update(super::viewer::Msg::Browse(hash)),
            MenuAction::CompareWithFiles(hash) => {
                let ignore_ws = self.diff_options.ignore_whitespace;
                if let Some(repo) = &mut self.repo {
                    repo.history.selected = Some(hash.clone());
                    repo.history.compare = None;
                }
                self.show_comparison(Comparison::WithWorkingTree { hash }, ignore_ws)
            }
            MenuAction::CherryPick(hash) => {
                let command = branches::cherry_pick(&root, &hash, is_merge(&hash));
                let label = t!("undo.cherry_pick", commit = short(&hash)).into_owned();
                self.perform(
                    Action::History,
                    Some(t!("busy.cherry_picking").into_owned()),
                    Some(Undoable::new(label, Moves::Integration)),
                    vec![command],
                )
            }
            MenuAction::Revert(hash) => {
                let command = branches::revert(&root, &hash, is_merge(&hash));
                let label = t!("undo.revert", commit = short(&hash)).into_owned();
                self.perform(
                    Action::History,
                    Some(t!("busy.reverting").into_owned()),
                    Some(Undoable::new(label, Moves::Integration)),
                    vec![command],
                )
            }
            MenuAction::Reword(hash) => {
                let message = repo
                    .history
                    .details
                    .as_ref()
                    .filter(|d| matches!(&d.comparison, Comparison::Commit { hash: h, .. } if *h == hash))
                    .map(|d| d.message.clone())
                    .or_else(|| {
                        repo.commits
                            .iter()
                            .find(|c| c.hash == hash)
                            .map(|c| c.subject.clone())
                    })
                    .unwrap_or_default();
                self.history_dialog = Some(Dialog::Reword {
                    commit: hash,
                    content: text_editor::Content::with_text(&message),
                });
                Task::none()
            }
            MenuAction::CopyId(hash) => {
                self.notice = Some(t!("history.copied", id = short(&hash)).into_owned());
                iced::clipboard::write(hash)
            }
            MenuAction::Switch(name) => self.switch_to(Target::Branch(local_name(repo, &name))),
            MenuAction::Merge(name) => self.open_integration(integrate::Kind::Merge, name),
            MenuAction::Rebase(name) => self.open_integration(integrate::Kind::Rebase, name),
            MenuAction::DeleteBranch(name) => self.delete_branch(name, false),
        }
    }

    /// Double-click on a branch: goes onto it, setting local changes aside
    /// for the branch being left and bringing back the ones of the target.
    pub(super) fn switch_to(&mut self, target: Target) -> Task<Message> {
        let (current, dirty) = self.current_branch_and_changes();
        // A server branch is followed by the local branch of the same name,
        // which `git switch` creates when it does not exist yet.
        let target = match (target, &self.repo) {
            (Target::Branch(name), Some(repo)) => Target::Branch(local_name(repo, &name)),
            (target, _) => target,
        };
        if let Target::Branch(name) = &target
            && current.as_deref() == Some(name.as_str())
        {
            return Task::none();
        }
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        // Git refuses a branch open in another workspace: say so without
        // setting the changes aside for nothing.
        if let Target::Branch(name) = &target
            && let Some(other) = repo
                .worktrees
                .holding(name)
                .filter(|t| t.path != repo.paths.root)
        {
            let path = other.path.clone();
            self.error = Some(
                t!(
                    "workspaces.switch_refused",
                    folder = super::folder_name(&path)
                )
                .into_owned(),
            );
            self.elsewhere = Some(path);
            return Task::none();
        }
        let root = repo.paths.root.clone();
        let shown = match &target {
            Target::Branch(name) => name.clone(),
            Target::Commit(hash) => hash.chars().take(7).collect(),
        };
        let mut undoable = Undoable::new(
            t!("undo.switch", branch = shown).into_owned(),
            Moves::Switch { set_aside: true },
        );
        if let Target::Branch(name) = &target {
            undoable.refs = vec![format!("refs/heads/{name}")];
        }
        self.perform_with(
            Action::History,
            Some(t!("busy.switching", branch = shown).into_owned()),
            Some(undoable),
            async move { branches::switch(&root, current.as_deref(), &target, dirty).await },
        )
    }

    pub(super) fn delete_branch(&mut self, name: String, force: bool) -> Task<Message> {
        self.history_dialog = None;
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let command = branches::delete_branch(&repo.paths.root, &name, force);
        let mut undoable = Undoable::new(
            t!("undo.delete_branch", branch = name).into_owned(),
            Moves::Index,
        );
        undoable.refs = vec![format!("refs/heads/{name}")];
        self.pending_delete = Some(name);
        self.perform(Action::DeleteBranch, None, Some(undoable), vec![command])
    }

    /// After `git branch -d` failed: asks before deleting for good.
    pub(super) fn delete_refused(&mut self) {
        if let Some(branch) = self.pending_delete.take() {
            self.error = None;
            self.history_dialog = Some(Dialog::ForceDelete { branch });
        }
    }

    pub(super) fn confirm_dialog(&mut self) -> Task<Message> {
        let Some(dialog) = self.history_dialog.take() else {
            return Task::none();
        };
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        match dialog {
            Dialog::NewBranch { at, name, switch } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let command = branches::create_branch(&root, &name, &at, switch);
                let (label, moves) = if switch {
                    (
                        t!("undo.new_branch_switch", branch = name),
                        Moves::Switch { set_aside: false },
                    )
                } else {
                    (t!("undo.new_branch", branch = name), Moves::Index)
                };
                let mut undoable = Undoable::new(label.into_owned(), moves);
                undoable.refs = vec![format!("refs/heads/{name}")];
                self.perform(Action::History, None, Some(undoable), vec![command])
            }
            Dialog::NewTag { at, name } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let command = branches::create_tag(&root, &name, &at);
                let mut undoable =
                    Undoable::new(t!("undo.new_tag", tag = name).into_owned(), Moves::Index);
                undoable.refs = vec![format!("refs/tags/{name}")];
                self.perform(Action::History, None, Some(undoable), vec![command])
            }
            Dialog::Reword { commit, content } => {
                let message = content.text().trim().to_string();
                if message.is_empty() {
                    return Task::none();
                }
                let head = repo.status.as_ref().and_then(|s| s.branch.oid.as_deref());
                if head != Some(commit.as_str()) {
                    return self.rewrite(
                        crate::git::rewrite::Edit::Reword {
                            commit,
                            message: format!("{message}\n"),
                        },
                        t!("undo.reword").into_owned(),
                    );
                }
                let command = branches::reword_head(&root, &message);
                let undoable = Undoable::new(t!("undo.reword").into_owned(), Moves::Commit);
                self.perform(Action::History, None, Some(undoable), vec![command])
            }
            Dialog::Drop { plan, squash } => self.run_plan(plan, squash),
            Dialog::ForceDelete { branch } => self.delete_branch(branch, true),
        }
    }

    /// Takes a file back to how it was in the commit shown.
    pub(super) fn restore_file(&mut self, hash: String, path: String) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let command = branches::restore_file(&root, &hash, &path);
        let short: String = hash.chars().take(7).collect();
        let mut undoable = Undoable::new(
            t!("undo.restore_file", path = path, commit = short).into_owned(),
            Moves::Index,
        );
        undoable.watch_file = Some(root.join(&path));
        self.perform(Action::History, None, Some(undoable), vec![command])
    }

    // -------------------------------------------------------------- views

    pub(super) fn history_view<'a>(
        &'a self,
        repo: &'a OpenRepo,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let history = &repo.history;
        let header = row![
            text(t!("history.title"))
                .size(15)
                .font(sans(Weight::Semibold))
                .width(Length::Fill),
            text_input(&t!("history.search"), &history.search)
                .size(13)
                .padding([6, 10])
                .width(300)
                .on_input(Message::HistorySearch)
                .style(style::input),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let head = repo.status.as_ref().and_then(|s| s.branch.oid.clone());
        let detached = repo
            .status
            .as_ref()
            .is_some_and(|s| s.branch.head.is_none() && s.branch.oid.is_some());
        let query = history.search.trim().to_lowercase();
        let searching = !query.is_empty();
        let graph_width = if searching {
            0
        } else {
            history.rows.iter().map(|r| r.width).max().unwrap_or(1)
        };

        let mut list = column![];
        if repo.commits.is_empty() {
            list = list.push(container(text(t!("history.none")).color(p.muted)).padding([16, 14]));
        }
        for (index, commit) in repo.commits.iter().enumerate() {
            if searching
                && !commit.subject.to_lowercase().contains(&query)
                && !commit.author.to_lowercase().contains(&query)
                && !commit.hash.starts_with(&query)
            {
                continue;
            }
            let graph = (!searching).then(|| history.rows.get(index)).flatten();
            let is_head = head.as_deref() == Some(commit.hash.as_str());
            list = list.push(self.commit_row(
                repo,
                commit,
                graph,
                graph_width,
                is_head,
                is_head && detached,
                p,
            ));
        }

        let card = container(column![
            container(header).padding([10, 14]),
            hline(style::separator),
            scrollable(list).style(style::scroll).height(Length::Fill),
            hline(style::separator),
            container(text(t!("history.tip")).size(12).color(p.muted)).padding([8, 14]),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::card);

        match &history.details {
            Some(details) => row![card, self.details_panel(repo, details, p)]
                .spacing(12)
                .height(Length::Fill)
                .into(),
            None => card.into(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_row<'a>(
        &'a self,
        repo: &'a OpenRepo,
        commit: &'a Commit,
        graph: Option<&'a graph::Row>,
        graph_width: usize,
        is_head: bool,
        detached_here: bool,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let history = &repo.history;
        let selected = history.selected.as_deref() == Some(commit.hash.as_str())
            || history.compare.as_deref() == Some(commit.hash.as_str());
        let current = repo.status.as_ref().and_then(|s| s.branch.head.as_deref());

        let mut line = row![]
            .spacing(8)
            .align_y(Alignment::Center)
            .height(Length::Fill);
        if let Some(graph) = graph {
            line = line.push(
                Canvas::new(RowGraph::new(graph, is_head, p))
                    .width(LANE_OFFSET * 2.0 + (graph_width.max(1) - 1) as f32 * LANE_WIDTH)
                    .height(ROW_HEIGHT),
            );
        } else {
            line = line.push(Space::new().width(6));
        }

        let mut subject = row![].spacing(6).align_y(Alignment::Center);
        if detached_here {
            subject = subject.push(
                container(
                    text(t!("history.you_are_here"))
                        .size(11)
                        .font(sans(Weight::Semibold)),
                )
                .padding([1, 7])
                .style(style::dashed_badge(p.warning)),
            );
        }
        for name in &commit.refs {
            if name == "HEAD" || name.ends_with("/HEAD") {
                continue;
            }
            subject = subject.push(self.ref_badge(repo, commit, name, current, p));
        }
        subject = subject.push(
            text(commit.subject.clone())
                .size(13)
                .wrapping(iced::widget::text::Wrapping::None),
        );

        line = line.push(container(subject).width(Length::Fill).clip(true));
        // With the details panel open the row is narrow: keep the message.
        if history.details.is_none() {
            line = line
                .push(
                    text(commit.author.clone())
                        .size(12)
                        .color(p.text_2)
                        .width(130)
                        .wrapping(iced::widget::text::Wrapping::None),
                )
                .push(
                    text(commit.date.clone())
                        .size(12)
                        .font(style::MONO)
                        .color(p.muted)
                        .width(118),
                );
        }
        line = line.push(
            text(commit.short.clone())
                .size(12)
                .font(style::MONO)
                .color(p.muted)
                .width(70),
        );

        let row = container(line.padding(Padding::default().right(14)))
            .height(ROW_HEIGHT)
            .width(Length::Fill);
        let grip = super::dragdrop::Grip::Commit(commit.hash.clone());
        let drop_here = self.dragging().is_some()
            && history.hover.as_ref().is_some_and(|h| match h {
                super::dragdrop::Grip::Commit(hash) => *hash == commit.hash,
                super::dragdrop::Grip::Branch(name) => commit.refs.contains(name),
                super::dragdrop::Grip::Workspaces => false,
            });
        let row = if drop_here {
            row.style(style::drop_target)
        } else if selected {
            row.style(style::selected_row)
        } else if detached_here {
            row.style(style::tinted(p.warning))
        } else {
            row
        };
        mouse_area(row)
            .on_press(Message::Grab(grip.clone()))
            .on_enter(Message::Hover(grip.clone()))
            .on_exit(Message::Unhover(grip))
            .on_right_press(Message::OpenMenu(MenuTarget::Commit(commit.hash.clone())))
            .into()
    }

    fn ref_badge<'a>(
        &'a self,
        repo: &'a OpenRepo,
        commit: &'a Commit,
        name: &'a str,
        current: Option<&str>,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        if let Some(tag) = name.strip_prefix("tag: ") {
            return container(
                row![
                    icon(Icon::Tag, 11.0, p.text_2),
                    text(tag.to_string())
                        .size(11)
                        .font(sans(Weight::Semibold))
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([1, 7])
            .style(style::badge(p.surface_alt, p.text_2))
            .into();
        }
        let remote = repo.branches.iter().any(|b| b.remote && b.name == name);
        let tint = style::branch_tint(p, name);
        let label = if current == Some(name) {
            t!("history.branch_here", branch = name).into_owned()
        } else {
            name.to_string()
        };
        let badge = container(
            text(label)
                .size(11)
                .font(sans(Weight::Semibold))
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .padding([1, 7]);
        let badge = if remote {
            badge.style(style::outlined_badge(tint))
        } else {
            badge.style(style::badge(tint.strong, style::text_on(tint.strong)))
        };
        tooltip(
            mouse_area(badge)
                .on_press(Message::Grab(super::dragdrop::Grip::Branch(
                    name.to_string(),
                )))
                .on_enter(Message::Hover(super::dragdrop::Grip::Branch(
                    name.to_string(),
                )))
                .on_exit(Message::LeaveBadge(name.to_string(), commit.hash.clone()))
                .on_double_click(Message::MenuAction(MenuAction::Switch(name.to_string())))
                .on_right_press(Message::OpenMenu(MenuTarget::Branch(name.to_string()))),
            container(text(t!("history.badge_tip")).size(12))
                .padding([4, 8])
                .style(style::card),
            tooltip::Position::Top,
        )
        .into()
    }

    fn details_panel<'a>(
        &'a self,
        repo: &'a OpenRepo,
        details: &'a Details,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let find = |hash: &str| repo.commits.iter().find(|c| c.hash == hash);
        let short = |hash: &str| hash.chars().take(7).collect::<String>();
        let mut head = column![].spacing(4);
        match &details.comparison {
            Comparison::Commit { hash, .. } => {
                let mut lines = details.message.lines();
                let title = lines.next().unwrap_or("").to_string();
                let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
                head = head.push(text(title).size(15).font(sans(Weight::Semibold)));
                if !body.is_empty() {
                    head = head.push(text(body).size(13).color(p.text_2));
                }
                if let Some(commit) = find(hash) {
                    head = head.push(
                        text(format!(
                            "{} · {} · {}",
                            commit.short, commit.author, commit.date
                        ))
                        .size(12)
                        .font(style::MONO)
                        .color(p.muted),
                    );
                }
                head = head.push(
                    button(centered(text(t!("viewer.browse")).size(12)))
                        .padding([0, 10])
                        .height(26)
                        .on_press(Message::Viewer(super::viewer::Msg::Browse(hash.clone())))
                        .style(style::secondary),
                );
            }
            Comparison::Between { from, to } => {
                head = head
                    .push(
                        text(t!("history.comparing"))
                            .size(15)
                            .font(sans(Weight::Semibold)),
                    )
                    .push(
                        text(format!("{} → {}", short(from), short(to)))
                            .size(12)
                            .font(style::MONO)
                            .color(p.muted),
                    );
            }
            Comparison::WithWorkingTree { hash } => {
                head = head
                    .push(
                        text(t!("history.compare_with_files"))
                            .size(15)
                            .font(sans(Weight::Semibold)),
                    )
                    .push(
                        text(t!("history.from_commit_to_files", commit = short(hash)))
                            .size(12)
                            .color(p.muted),
                    );
            }
        }
        let close = button(container(icon(Icon::Cross, 14.0, p.text_2)).center(Length::Fill))
            .width(28)
            .height(28)
            .padding(0)
            .on_press(Message::CloseDetails)
            .style(style::secondary);
        let header = row![head.width(Length::Fill), close].spacing(8);

        let mut files = column![].spacing(2);
        if details.files.is_empty() {
            files = files.push(text(t!("history.no_files")).size(13).color(p.muted));
        }
        for file in &details.files {
            let (letter, tint) = match file.status {
                'A' => ("+", p.added),
                'D' => ("−", p.removed),
                'R' | 'C' => ("→", p.moved),
                _ => ("~", p.accent),
            };
            let selected = details.file.as_deref() == Some(file.path.as_str());
            let mut label = row![
                container(
                    text(letter)
                        .size(12)
                        .font(mono(Weight::Medium))
                        .color(tint.strong)
                )
                .width(16),
                text(file.path.clone()).size(12).font(style::MONO),
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            if let Some(old) = &file.old_path {
                label = label.push(
                    text(t!("history.renamed_from", path = old))
                        .size(11)
                        .color(p.muted),
                );
            }
            files = files.push(
                button(label)
                    .padding([4, 8])
                    .width(Length::Fill)
                    .on_press(Message::SelectCommitFile(file.path.clone()))
                    .style(style::choice(selected)),
            );
        }

        let mut diff_col = column![];
        if let (Some(path), Some(diff)) = (&details.file, &details.diff) {
            let mut toolbar = row![
                text(path.clone())
                    .size(12)
                    .font(mono(Weight::Medium))
                    .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if !diff.binary && !diff.is_empty() {
                toolbar = toolbar.push(
                    button(centered(text(t!("viewer.open_large")).size(12)))
                        .padding([0, 10])
                        .height(26)
                        .on_press(Message::Viewer(super::viewer::Msg::OpenCommitFile))
                        .style(style::secondary),
                );
            }
            if let Comparison::Commit { hash, .. } = &details.comparison {
                let deleted = details
                    .files
                    .iter()
                    .any(|f| f.path == *path && f.status == 'D');
                if !deleted {
                    toolbar = toolbar.push(
                        button(centered(text(t!("history.restore_file")).size(12)))
                            .padding([0, 10])
                            .height(26)
                            .on_press_maybe(
                                self.idle()
                                    .then(|| Message::RestoreFile(hash.clone(), path.clone())),
                            )
                            .style(style::secondary),
                    );
                }
            }
            diff_col = diff_col.push(container(toolbar).padding([6, 10]));
            if let Some(command) = &details.command {
                diff_col = diff_col.push(
                    container(
                        text(format!("$ {command}"))
                            .size(11)
                            .font(style::MONO)
                            .color(p.muted),
                    )
                    .padding(Padding::from([0, 10]).bottom(6)),
                );
            }
            if diff.binary {
                diff_col =
                    diff_col.push(container(text(t!("diff.binary")).color(p.muted)).padding(14));
            } else if diff.is_empty() {
                diff_col = diff_col
                    .push(container(text(t!("history.no_text_change")).color(p.muted)).padding(14));
            }
            let mut budget = MAX_DIFF_LINES;
            let lang = crate::ui::syntax::language_for(path);
            for hunk in &diff.hunks {
                let (from, to) = hunk_range(hunk);
                let mut title = text(t!("diff.lines", from = from, to = to)).size(12);
                if !hunk.section.is_empty() {
                    title = text(format!(
                        "{} · {}",
                        hunk.section,
                        t!("diff.lines", from = from, to = to)
                    ))
                    .size(12);
                }
                diff_col = diff_col.push(
                    container(title)
                        .padding([4, 10])
                        .width(Length::Fill)
                        .style(style::hunk_header),
                );
                let shown = hunk.lines.len().min(budget);
                diff_col = diff_col.push(if self.side_by_side {
                    side_by_side(&hunk.lines[..shown], lang, p)
                } else {
                    unified(&hunk.lines[..shown], lang, p)
                });
                budget -= shown;
                if budget == 0 {
                    break;
                }
            }
        }

        container(
            column![
                container(header).padding([12, 14]),
                hline(style::separator),
                container(scrollable(files.padding([6, 8])).style(style::scroll)).max_height(180),
                hline(style::separator),
                scrollable(diff_col)
                    .style(style::scroll)
                    .height(Length::Fill),
            ]
            .height(Length::Fill),
        )
        .width(DETAILS_WIDTH)
        .height(Length::Fill)
        .style(style::card)
        .into()
    }

    /// The menu drawn where the right-click happened.
    pub(super) fn context_menu_view<'a>(
        &'a self,
        menu: &'a ContextMenu,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let Some(repo) = &self.repo else {
            return Space::new().into();
        };
        let current = repo.status.as_ref().and_then(|s| s.branch.head.clone());
        let enabled = self.idle() && repo.operation.is_none();
        let mut items = column![].spacing(0);
        let mut count = 0usize;
        let mut separators = 0usize;
        let mut add = |items: iced::widget::Column<'a, Message>,
                       label: String,
                       verb: &'static str,
                       action: Option<MenuAction>,
                       strong: bool| {
            count += 1;
            let label_text = text(label).size(13).font(sans(if strong {
                Weight::Semibold
            } else {
                Weight::Normal
            }));
            items.push(
                button(
                    row![
                        label_text.width(Length::Fill),
                        text(verb).size(11).font(style::MONO).color(p.muted),
                    ]
                    .spacing(12)
                    .height(Length::Fill)
                    .align_y(Alignment::Center),
                )
                .padding([0, 10])
                .height(MENU_ITEM_HEIGHT)
                .width(Length::Fill)
                .on_press_maybe(action.map(Message::MenuAction))
                .style(style::menu_item),
            )
        };
        let separator = |items: iced::widget::Column<'a, Message>| {
            items.push(container(hline(style::separator)).padding([4, 0]))
        };

        let title = match &menu.target {
            MenuTarget::Commit(hash) => {
                let commit = repo.commits.iter().find(|c| c.hash == *hash);
                let pushed = commit.is_some_and(|c| is_on_remote(repo, &c.hash));
                let is_head = repo.status.as_ref().and_then(|s| s.branch.oid.as_deref())
                    == Some(hash.as_str());
                let on = |a: MenuAction| enabled.then_some(a);
                items = add(
                    items,
                    t!("menu.new_branch").into_owned(),
                    "switch -c",
                    on(MenuAction::NewBranch(hash.clone())),
                    true,
                );
                items = add(
                    items,
                    t!("menu.new_tag").into_owned(),
                    "tag",
                    on(MenuAction::NewTag(hash.clone())),
                    false,
                );
                items = separator(items);
                separators += 1;
                items = add(
                    items,
                    t!("menu.show").into_owned(),
                    "show",
                    Some(MenuAction::ShowCommit(hash.clone())),
                    false,
                );
                items = add(
                    items,
                    t!("menu.compare_files").into_owned(),
                    "diff",
                    Some(MenuAction::CompareWithFiles(hash.clone())),
                    false,
                );
                items = add(
                    items,
                    t!("menu.browse").into_owned(),
                    "ls-tree",
                    Some(MenuAction::Browse(hash.clone())),
                    false,
                );
                items = separator(items);
                separators += 1;
                let onto = current
                    .clone()
                    .unwrap_or_else(|| t!("menu.here").into_owned());
                items = add(
                    items,
                    t!("menu.cherry_pick", branch = onto).into_owned(),
                    "cherry-pick",
                    on(MenuAction::CherryPick(hash.clone())).filter(|_| !is_head),
                    false,
                );
                items = add(
                    items,
                    t!("menu.revert").into_owned(),
                    "revert",
                    on(MenuAction::Revert(hash.clone())),
                    false,
                );
                let rewordable = self.can_reword(hash);
                let reword = if pushed {
                    t!("menu.reword_pushed").into_owned()
                } else if !rewordable {
                    t!("menu.reword_not_here").into_owned()
                } else {
                    t!("menu.reword").into_owned()
                };
                items = add(
                    items,
                    reword,
                    if is_head {
                        "commit --amend"
                    } else {
                        "rebase -i"
                    },
                    on(MenuAction::Reword(hash.clone())).filter(|_| rewordable && !pushed),
                    false,
                );
                items = separator(items);
                separators += 1;
                items = add(
                    items,
                    t!("menu.copy_id").into_owned(),
                    "Ctrl+C",
                    Some(MenuAction::CopyId(hash.clone())),
                    false,
                );
                match commit {
                    Some(c) => format!("{} · {} · {}", c.short, c.author, c.date),
                    None => hash.chars().take(7).collect(),
                }
            }
            MenuTarget::Branch(name) => {
                let local = !repo.branches.iter().any(|b| b.remote && b.name == *name);
                let is_current = current.as_deref() == Some(name.as_str());
                let follows = local_name(repo, name);
                let on = |a: MenuAction| enabled.then_some(a);
                items = add(
                    items,
                    t!("menu.switch", branch = follows).into_owned(),
                    "switch",
                    on(MenuAction::Switch(name.clone()))
                        .filter(|_| current.as_deref() != Some(follows.as_str())),
                    true,
                );
                if let Some(current) = current.as_ref().filter(|_| !is_current) {
                    items = separator(items);
                    separators += 1;
                    items = add(
                        items,
                        t!("menu.merge", branch = name, into = current).into_owned(),
                        "merge",
                        on(MenuAction::Merge(name.clone())),
                        false,
                    );
                    items = add(
                        items,
                        t!("menu.rebase", branch = current, onto = name).into_owned(),
                        "rebase",
                        on(MenuAction::Rebase(name.clone())),
                        false,
                    );
                }
                if local {
                    items = separator(items);
                    separators += 1;
                    items = add(
                        items,
                        t!("menu.delete_branch", branch = name).into_owned(),
                        "branch -d",
                        on(MenuAction::DeleteBranch(name.clone())).filter(|_| !is_current),
                        false,
                    );
                }
                t!("menu.branch_title", branch = name).into_owned()
            }
        };

        let panel = container(
            column![
                container(text(title).size(12).color(p.muted)).padding([6, 10]),
                hline(style::separator),
                items.padding([4, 0]),
            ]
            .width(MENU_WIDTH),
        )
        .padding(4)
        .style(style::popup);

        // Keep the menu inside the window.
        let height = 40.0 + count as f32 * MENU_ITEM_HEIGHT + separators as f32 * 9.0 + 16.0;
        let x = menu
            .at
            .x
            .min(self.window.width - MENU_WIDTH - 20.0)
            .max(0.0);
        let y = menu.at.y.min(self.window.height - height - 12.0).max(0.0);
        container(panel)
            .padding(Padding::default().left(x).top(y))
            .into()
    }

    pub(super) fn history_dialog_view<'a>(
        &'a self,
        dialog: &'a Dialog,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let Some(repo) = &self.repo else {
            return Space::new().into();
        };
        let short = |hash: &str| hash.chars().take(7).collect::<String>();
        let (title, intro, command, confirm, body): (
            String,
            String,
            String,
            String,
            Element<'a, Message>,
        ) = match dialog {
            Dialog::NewBranch { at, name, switch } => {
                let shown = if name.trim().is_empty() {
                    "<name>"
                } else {
                    name.trim()
                };
                let command =
                    branches::create_branch(&repo.paths.root, shown, &short(at), *switch).display();
                (
                    t!("dialog.new_branch_title").into_owned(),
                    t!("dialog.new_branch_intro", commit = short(at)).into_owned(),
                    command,
                    t!("dialog.create").into_owned(),
                    column![
                        text_input(&t!("dialog.branch_placeholder"), name)
                            .id("dialog-name")
                            .font(style::MONO)
                            .size(13)
                            .padding([8, 10])
                            .on_input(Message::DialogName)
                            .on_submit(Message::ConfirmDialog)
                            .style(style::input),
                        checkbox(*switch)
                            .label(t!("dialog.switch_to_it"))
                            .size(16)
                            .text_size(13)
                            .on_toggle(Message::DialogSwitch)
                            .style(style::check),
                    ]
                    .spacing(10)
                    .into(),
                )
            }
            Dialog::NewTag { at, name } => {
                let shown = if name.trim().is_empty() {
                    "<name>"
                } else {
                    name.trim()
                };
                (
                    t!("dialog.new_tag_title").into_owned(),
                    t!("dialog.new_tag_intro", commit = short(at)).into_owned(),
                    branches::create_tag(&repo.paths.root, shown, &short(at)).display(),
                    t!("dialog.create").into_owned(),
                    text_input(&t!("dialog.tag_placeholder"), name)
                        .id("dialog-name")
                        .font(style::MONO)
                        .size(13)
                        .padding([8, 10])
                        .on_input(Message::DialogName)
                        .on_submit(Message::ConfirmDialog)
                        .style(style::input)
                        .into(),
                )
            }
            Dialog::Reword { commit, content } => (
                t!("dialog.reword_title").into_owned(),
                t!("dialog.reword_intro").into_owned(),
                if repo.status.as_ref().and_then(|s| s.branch.oid.as_deref())
                    == Some(commit.as_str())
                {
                    "git commit --amend --only -m <message>".to_string()
                } else {
                    let edit = crate::git::rewrite::Edit::Reword {
                        commit: commit.clone(),
                        message: String::new(),
                    };
                    let base = self
                        .rewrite_parts(&edit)
                        .and_then(|(base, _)| base)
                        .map(|b| short(&b));
                    format!(
                        "{}\n  reword {}",
                        crate::git::rewrite::shown(&repo.paths.root, base.as_deref()),
                        short(commit)
                    )
                },
                t!("dialog.save").into_owned(),
                text_editor(content)
                    .height(140)
                    .padding(10)
                    .size(13)
                    .on_action(Message::DialogEdit)
                    .style(style::editor)
                    .into(),
            ),
            Dialog::Drop { plan, squash } => {
                let (title, intro, confirm) = self.drop_texts(plan, *squash);
                let body: Element<'a, Message> = match plan {
                    super::dragdrop::DropPlan::Rewrite { .. } => row![
                        button(centered(text(t!("drop.swap"))))
                            .padding([0, 14])
                            .height(36)
                            .on_press(Message::DialogSquash(false))
                            .style(style::choice(!*squash)),
                        button(centered(text(t!("drop.squash"))))
                            .padding([0, 14])
                            .height(36)
                            .on_press(Message::DialogSquash(true))
                            .style(style::choice(*squash)),
                    ]
                    .spacing(8)
                    .into(),
                    _ => Space::new().into(),
                };
                (
                    title,
                    intro,
                    self.plan_commands(plan, *squash).join("\n"),
                    confirm,
                    body,
                )
            }
            Dialog::ForceDelete { branch } => (
                t!("dialog.force_delete_title", branch = branch).into_owned(),
                t!("dialog.force_delete_intro", branch = branch).into_owned(),
                branches::delete_branch(&repo.paths.root, branch, true).display(),
                t!("dialog.delete_anyway").into_owned(),
                text(t!("dialog.force_delete_undo"))
                    .size(13)
                    .color(p.text_2)
                    .into(),
            ),
        };
        let destructive = matches!(dialog, Dialog::ForceDelete { .. });
        let undo_note = match dialog {
            Dialog::Drop {
                plan: super::dragdrop::DropPlan::Push { .. },
                ..
            } => t!("drop.push_not_undoable"),
            _ => t!("dialog.undoable"),
        };
        container(
            column![
                text(title).size(20).font(sans(Weight::Semibold)),
                text(intro).color(p.text_2),
                body,
                container(
                    row![
                        text("$").font(style::MONO).size(12).color(jc::PROMPT),
                        text(command).font(style::MONO).size(12),
                    ]
                    .spacing(8),
                )
                .padding([8, 12])
                .width(Length::Fill)
                .style(style::journal_box),
                row![
                    text(undo_note).size(12).color(p.muted),
                    space::horizontal(),
                    button(centered(text(t!("common.cancel"))))
                        .padding([0, 16])
                        .height(40)
                        .on_press(Message::CloseDialog)
                        .style(style::secondary),
                    button(centered(text(confirm).font(sans(Weight::Semibold))))
                        .padding([0, 16])
                        .height(40)
                        .on_press_maybe(self.idle().then_some(Message::ConfirmDialog))
                        .style(if destructive {
                            style::danger
                        } else {
                            style::primary
                        }),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .spacing(14),
        )
        .padding(24)
        .max_width(560)
        .style(style::dialog)
        .into()
    }
}

/// `origin/feature/x` → `feature/x`; local names are kept.
pub(super) fn local_name(repo: &OpenRepo, name: &str) -> String {
    match repo.branches.iter().find(|b| b.remote && b.name == name) {
        Some(_) => name
            .split_once('/')
            .map_or(name, |(_, rest)| rest)
            .to_string(),
        None => name.to_string(),
    }
}

/// Whether a remote branch contains the commit, as far as the history shown
/// tells: a remote branch points at it or at one of its descendants.
pub(super) fn is_on_remote(repo: &OpenRepo, hash: &str) -> bool {
    let remote_tips: Vec<&str> = repo
        .commits
        .iter()
        .filter(|c| {
            c.refs
                .iter()
                .any(|r| repo.branches.iter().any(|b| b.remote && b.name == *r))
        })
        .map(|c| c.hash.as_str())
        .collect();
    // Walk down from each remote tip.
    let by_hash: HashMap<&str, &Commit> =
        repo.commits.iter().map(|c| (c.hash.as_str(), c)).collect();
    let mut stack = remote_tips;
    let mut seen = std::collections::HashSet::new();
    while let Some(h) = stack.pop() {
        if h == hash {
            return true;
        }
        if !seen.insert(h) {
            continue;
        }
        if let Some(c) = by_hash.get(h) {
            stack.extend(c.parents.iter().map(String::as_str));
        }
    }
    false
}

/// Draws one row of the graph.
struct RowGraph {
    segments: Vec<(Half, usize, usize, Color)>,
    lane: usize,
    dot: Color,
    head: bool,
    surface: Color,
}

impl RowGraph {
    fn new(row: &graph::Row, head: bool, p: &'static Palette) -> Self {
        let color = |owner: &Option<String>| match owner {
            Some(name) => style::branch_tint(p, name).strong,
            None => p.muted,
        };
        Self {
            segments: row
                .segments
                .iter()
                .map(|s| (s.half, s.from, s.to, color(&s.owner)))
                .collect(),
            lane: row.lane,
            dot: color(&row.owner),
            head,
            surface: p.surface,
        }
    }
}

impl canvas::Program<Message> for RowGraph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let mid = bounds.height / 2.0;
        let x = |lane: usize| LANE_OFFSET + lane as f32 * LANE_WIDTH;
        for (half, from, to, color) in &self.segments {
            let (y0, y1) = match half {
                Half::Top => (0.0, mid),
                Half::Bottom => (mid, bounds.height),
            };
            let path = Shape::new(|b| {
                b.move_to(Point::new(x(*from), y0));
                if from == to {
                    b.line_to(Point::new(x(*to), y1));
                } else {
                    let middle = (y0 + y1) / 2.0;
                    b.bezier_curve_to(
                        Point::new(x(*from), middle),
                        Point::new(x(*to), middle),
                        Point::new(x(*to), y1),
                    );
                }
            });
            frame.stroke(&path, Stroke::default().with_color(*color).with_width(2.5));
        }
        let center = Point::new(x(self.lane), mid);
        if self.head {
            frame.fill(&Shape::circle(center, 8.0), self.surface);
            frame.stroke(
                &Shape::circle(center, 8.0),
                Stroke::default().with_color(self.dot).with_width(3.0),
            );
        } else {
            frame.fill(&Shape::circle(center, 5.5), self.dot);
        }
        vec![frame.into_geometry()]
    }
}
