//! The "Workspaces" screen: git worktrees, several branches checked out side
//! by side in their own folders, all sharing one repository.

use iced::widget::{column, pick_list, row};

use super::modules::small_button;
use super::undo::Undoable;
use super::*;
use crate::git::undo::{Inverse, Moves};
use crate::git::worktrees::{self, Start, Worktree};

const LOCK_INPUT: &str = "workspace-lock";
const NAME_INPUT: &str = "workspace-name";
const COMMIT_INPUT: &str = "workspace-commit";

#[derive(Default)]
pub struct WorktreesState {
    /// `None` until read.
    pub list: Option<Vec<Worktree>>,
    pub form: Option<Form>,
    /// The workspace whose lock reason is being typed.
    pub locking: Option<PathBuf>,
    pub lock_reason: String,
    /// A removal that would lose changes, waiting for confirmation.
    pub confirm_force: Option<PathBuf>,
}

impl WorktreesState {
    /// How many workspaces need a look (the sidebar badge): folders gone.
    pub fn attention(&self) -> usize {
        self.list
            .as_ref()
            .map_or(0, |l| l.iter().filter(|t| t.missing).count())
    }

    /// The workspace that has `branch` checked out, if any.
    pub fn holding(&self, branch: &str) -> Option<&Worktree> {
        self.list
            .as_ref()?
            .iter()
            .find(|t| t.branch.as_deref() == Some(branch))
    }

    fn main_folder(&self) -> Option<&Path> {
        self.list
            .as_ref()?
            .iter()
            .find(|t| t.main)
            .map(|t| t.path.as_path())
    }
}

/// What a new workspace starts on, as chosen in the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Branch,
    NewBranch,
    Commit,
}

/// The "New workspace" form.
#[derive(Debug, Clone, Default)]
pub struct Form {
    pub kind: Kind,
    /// The existing branch to open.
    pub branch: Option<String>,
    /// The name of the branch to create.
    pub name: String,
    /// Where the new branch starts, or the commit to open.
    pub at: String,
    pub folder: String,
    /// The folder was typed or picked: stop suggesting one.
    pub folder_chosen: bool,
    pub submodules: bool,
}

impl Form {
    pub fn start(&self) -> Option<Start> {
        match self.kind {
            Kind::Branch => self.branch.clone().map(Start::Branch),
            Kind::NewBranch => {
                let name = self.name.trim();
                let at = self.at.trim();
                (crate::git::remotes::valid_name(name) && !at.is_empty()).then(|| {
                    Start::NewBranch {
                        name: name.to_string(),
                        at: at.to_string(),
                    }
                })
            }
            Kind::Commit => {
                let at = self.at.trim();
                (!at.is_empty()).then(|| Start::Detached(at.to_string()))
            }
        }
    }

    /// The name the suggested folder is made from.
    fn label(&self) -> String {
        match self.kind {
            Kind::Branch => self.branch.clone().unwrap_or_default(),
            Kind::NewBranch => self.name.trim().to_string(),
            Kind::Commit => self.at.trim().chars().take(7).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Loaded(PathBuf, Vec<Worktree>, Vec<CommandRecord>),
    Open(PathBuf),
    /// Opens the form, filled in when a branch or commit was dropped.
    New(Option<Start>),
    CloseForm,
    Kind(Kind),
    Branch(String),
    Name(String),
    At(String),
    Folder(String),
    PickFolder,
    FolderPicked(Option<PathBuf>),
    Submodules(bool),
    Create,
    /// The commit a new branch starts on, read before creating it.
    Resolved(Option<String>, CommandRecord),
    Remove(PathBuf),
    /// Asks to remove a workspace with changes (advanced mode).
    ForceRemove(PathBuf),
    ConfirmForce,
    CancelForce,
    Prune,
    StartLock(PathBuf),
    LockReason(String),
    Lock,
    CancelLock,
    Unlock(PathBuf),
    DownloadSubmodules(PathBuf),
}

impl App {
    pub(super) fn load_worktrees(&mut self) -> Task<Message> {
        let Some(root) = self.repo.as_ref().map(|r| r.paths.root.clone()) else {
            return Task::none();
        };
        Task::perform(
            async move {
                let (list, records) = worktrees::read(&root).await;
                Message::Worktrees(Msg::Loaded(root, list, records))
            },
            |message| message,
        )
    }

    /// Shows the "New workspace" form, e.g. after a branch was dropped on
    /// the Workspaces item.
    pub(super) fn new_workspace(&mut self, start: Option<Start>) -> Task<Message> {
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let mut form = Form {
            submodules: repo.paths.root.join(".gitmodules").is_file(),
            ..Form::default()
        };
        let current = repo.status.as_ref().and_then(|s| s.branch.head.clone());
        match start {
            Some(Start::Branch(branch)) => form.branch = Some(branch),
            Some(Start::NewBranch { name, at }) => {
                form.kind = Kind::NewBranch;
                form.name = name;
                form.at = at;
            }
            Some(Start::Detached(commit)) => {
                form.kind = Kind::Commit;
                form.at = commit;
            }
            None => {
                // The first branch not open anywhere yet.
                form.branch = repo
                    .branches
                    .iter()
                    .filter(|b| !b.remote)
                    .map(|b| b.name.clone())
                    .find(|b| repo.worktrees.holding(b).is_none());
                form.at = current.unwrap_or_default();
            }
        }
        repo.worktrees.form = Some(form);
        repo.section = Section::Workspaces;
        self.suggest_folder();
        self.load_worktrees()
    }

    fn suggest_folder(&mut self) {
        let Some(repo) = &mut self.repo else {
            return;
        };
        let main = repo
            .worktrees
            .main_folder()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| repo.paths.root.clone());
        if let Some(form) = repo.worktrees.form.as_mut().filter(|f| !f.folder_chosen) {
            let label = form.label();
            form.folder = if label.is_empty() {
                String::new()
            } else {
                worktrees::suggested_folder(&main, &label)
                    .to_string_lossy()
                    .into_owned()
            };
        }
    }

    fn form_mut(&mut self) -> Option<&mut Form> {
        self.repo.as_mut()?.worktrees.form.as_mut()
    }

    pub(super) fn worktrees_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Loaded(root, list, records) => {
                for record in records {
                    self.log(record);
                }
                if let Some(repo) = self.repo.as_mut().filter(|r| r.paths.root == root) {
                    repo.worktrees.list = Some(list);
                    self.suggest_folder();
                }
                Task::none()
            }
            Msg::Open(path) => {
                self.elsewhere = None;
                self.error = None;
                self.open(path)
            }
            Msg::New(start) => self.new_workspace(start),
            Msg::CloseForm => {
                if let Some(repo) = &mut self.repo {
                    repo.worktrees.form = None;
                }
                Task::none()
            }
            Msg::Kind(kind) => {
                if let Some(form) = self.form_mut() {
                    form.kind = kind;
                }
                self.suggest_folder();
                match kind {
                    Kind::NewBranch => iced::widget::operation::focus(NAME_INPUT),
                    Kind::Commit => iced::widget::operation::focus(COMMIT_INPUT),
                    Kind::Branch => Task::none(),
                }
            }
            Msg::Branch(branch) => {
                // A server branch with no local branch yet: one is created.
                let remote_only = self.repo.as_ref().and_then(|r| {
                    let local = super::history::local_name(r, &branch);
                    (local != branch && !r.branches.iter().any(|b| !b.remote && b.name == local))
                        .then_some(local)
                });
                if let Some(form) = self.form_mut() {
                    match remote_only {
                        Some(local) => {
                            form.kind = Kind::NewBranch;
                            form.name = local;
                            form.at = branch;
                        }
                        None => form.branch = Some(branch),
                    }
                }
                self.suggest_folder();
                Task::none()
            }
            Msg::Name(name) => {
                if let Some(form) = self.form_mut() {
                    form.name = name;
                }
                self.suggest_folder();
                Task::none()
            }
            Msg::At(at) => {
                if let Some(form) = self.form_mut() {
                    form.at = at;
                }
                self.suggest_folder();
                Task::none()
            }
            Msg::Folder(folder) => {
                if let Some(form) = self.form_mut() {
                    form.folder_chosen = !folder.is_empty();
                    form.folder = folder;
                }
                self.suggest_folder();
                Task::none()
            }
            Msg::PickFolder => Task::perform(
                pick_folder(t!("workspaces.pick_folder").into_owned()),
                |dir| Message::Worktrees(Msg::FolderPicked(dir)),
            ),
            Msg::FolderPicked(dir) => {
                // A parent folder was picked: the workspace goes inside it.
                if let Some(dir) = dir
                    && let Some(form) = self.form_mut()
                {
                    let name = Path::new(&form.folder)
                        .file_name()
                        .map(|n| n.to_os_string())
                        .unwrap_or_else(|| "workspace".into());
                    form.folder = dir.join(name).to_string_lossy().into_owned();
                    form.folder_chosen = true;
                }
                Task::none()
            }
            Msg::Submodules(on) => {
                if let Some(form) = self.form_mut() {
                    form.submodules = on;
                }
                Task::none()
            }
            Msg::Create => {
                let Some(repo) = &self.repo else {
                    return Task::none();
                };
                let Some(form) = &repo.worktrees.form else {
                    return Task::none();
                };
                match form.start() {
                    // The branch is removed on undo only if it did not move
                    // since: its starting commit is read first.
                    Some(Start::NewBranch { at, .. }) => {
                        let command = GitCommand::new([
                            "rev-parse".to_string(),
                            "--verify".to_string(),
                            "--quiet".to_string(),
                            format!("{at}^{{commit}}"),
                        ])
                        .in_dir(&repo.paths.root)
                        .background();
                        Task::perform(run(command), |record| {
                            let oid = record
                                .success()
                                .then(|| record.stdout.trim().to_string())
                                .filter(|o| !o.is_empty());
                            Message::Worktrees(Msg::Resolved(oid, record))
                        })
                    }
                    Some(_) => self.create_workspace(None),
                    None => Task::none(),
                }
            }
            Msg::Resolved(oid, record) => {
                self.log(record);
                match oid {
                    Some(oid) => self.create_workspace(Some(oid)),
                    None => {
                        let at = self
                            .repo
                            .as_ref()
                            .and_then(|r| r.worktrees.form.as_ref())
                            .map(|f| f.at.trim().to_string())
                            .unwrap_or_default();
                        self.error = Some(t!("workspaces.unknown_start", at = at).into_owned());
                        Task::none()
                    }
                }
            }
            Msg::Remove(path) => self.remove_workspace(path, false),
            Msg::ForceRemove(path) => {
                if let Some(repo) = &mut self.repo {
                    repo.worktrees.confirm_force = Some(path);
                }
                Task::none()
            }
            Msg::ConfirmForce => {
                let Some(path) = self
                    .repo
                    .as_mut()
                    .and_then(|r| r.worktrees.confirm_force.take())
                else {
                    return Task::none();
                };
                self.remove_workspace(path, true)
            }
            Msg::CancelForce => {
                if let Some(repo) = &mut self.repo {
                    repo.worktrees.confirm_force = None;
                }
                Task::none()
            }
            Msg::Prune => {
                let Some(root) = self.repo.as_ref().map(|r| r.paths.root.clone()) else {
                    return Task::none();
                };
                self.perform(Action::Workspace, None, None, vec![worktrees::prune(&root)])
            }
            Msg::StartLock(path) => {
                if let Some(repo) = &mut self.repo {
                    repo.worktrees.locking = Some(path);
                    repo.worktrees.lock_reason.clear();
                }
                iced::widget::operation::focus(LOCK_INPUT)
            }
            Msg::LockReason(reason) => {
                if let Some(repo) = &mut self.repo {
                    repo.worktrees.lock_reason = reason;
                }
                Task::none()
            }
            Msg::CancelLock => {
                if let Some(repo) = &mut self.repo {
                    repo.worktrees.locking = None;
                }
                Task::none()
            }
            Msg::Lock => {
                let Some(repo) = &mut self.repo else {
                    return Task::none();
                };
                let Some(path) = repo.worktrees.locking.take() else {
                    return Task::none();
                };
                let root = repo.paths.root.clone();
                let reason = repo.worktrees.lock_reason.trim().to_string();
                let mut undoable = Undoable::new(
                    t!("undo.workspace_lock", folder = folder_name(&path)).into_owned(),
                    Moves::Index,
                );
                undoable.inverse = Some(Inverse {
                    undo: vec![worktrees::unlock(&root, &path)],
                    redo: vec![worktrees::lock(&root, &path, &reason)],
                });
                self.perform(
                    Action::Workspace,
                    None,
                    Some(undoable),
                    vec![worktrees::lock(&root, &path, &reason)],
                )
            }
            Msg::Unlock(path) => {
                let Some(repo) = &self.repo else {
                    return Task::none();
                };
                let root = repo.paths.root.clone();
                let reason = repo
                    .worktrees
                    .list
                    .as_ref()
                    .and_then(|l| l.iter().find(|t| t.path == path))
                    .and_then(|t| t.locked.clone())
                    .unwrap_or_default();
                let mut undoable = Undoable::new(
                    t!("undo.workspace_unlock", folder = folder_name(&path)).into_owned(),
                    Moves::Index,
                );
                undoable.inverse = Some(Inverse {
                    undo: vec![worktrees::lock(&root, &path, &reason)],
                    redo: vec![worktrees::unlock(&root, &path)],
                });
                self.perform(
                    Action::Workspace,
                    None,
                    Some(undoable),
                    vec![worktrees::unlock(&root, &path)],
                )
            }
            Msg::DownloadSubmodules(path) => self.perform(
                Action::Workspace,
                Some(t!("busy.workspace_submodules", folder = folder_name(&path)).into_owned()),
                None,
                vec![worktrees::submodules(&path)],
            ),
        }
    }

    /// Creates the workspace of the form. `start_oid`: the commit a new
    /// branch starts on.
    fn create_workspace(&mut self, start_oid: Option<String>) -> Task<Message> {
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let Some(form) = repo.worktrees.form.clone() else {
            return Task::none();
        };
        let Some(start) = form.start() else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let folder = PathBuf::from(form.folder.trim());
        let add = worktrees::add(&root, &folder, &start);
        let mut commands = vec![add.clone()];
        let submodules = form.submodules && root.join(".gitmodules").is_file();
        if submodules {
            commands.push(worktrees::submodules(&folder));
        }
        // A workspace with submodules can't be removed without forcing,
        // which could lose work: that one is not undone with Ctrl+Z.
        let undoable = (!submodules).then(|| {
            let mut undo = vec![worktrees::remove(&root, &folder, false)];
            if let (Start::NewBranch { name, .. }, Some(oid)) = (&start, &start_oid) {
                // Deleted only if no commit was added to it since.
                undo.push(
                    GitCommand::new([
                        "update-ref".to_string(),
                        "-d".to_string(),
                        format!("refs/heads/{name}"),
                        oid.clone(),
                    ])
                    .in_dir(&root),
                );
            }
            let mut undoable = Undoable::new(
                t!("undo.workspace_add", folder = folder_name(&folder)).into_owned(),
                Moves::Index,
            );
            undoable.inverse = Some(Inverse {
                undo,
                redo: vec![add],
            });
            undoable
        });
        repo.worktrees.form = None;
        self.perform(
            Action::Workspace,
            Some(t!("busy.workspace_add", folder = folder_name(&folder)).into_owned()),
            undoable,
            commands,
        )
    }

    fn remove_workspace(&mut self, path: PathBuf, force: bool) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let tree = repo
            .worktrees
            .list
            .as_ref()
            .and_then(|l| l.iter().find(|t| t.path == path))
            .cloned();
        // A clean workspace comes back on undo, on the same branch or commit.
        let again = tree.as_ref().and_then(|t| match (&t.branch, &t.head) {
            (Some(branch), _) => Some(Start::Branch(branch.clone())),
            (None, Some(head)) => Some(Start::Detached(head.clone())),
            _ => None,
        });
        let undoable = again.filter(|_| !force).map(|start| {
            let mut undoable = Undoable::new(
                t!("undo.workspace_remove", folder = folder_name(&path)).into_owned(),
                Moves::Index,
            );
            undoable.inverse = Some(Inverse {
                undo: vec![worktrees::add(&root, &path, &start)],
                redo: vec![worktrees::remove(&root, &path, false)],
            });
            undoable
        });
        self.perform(
            Action::Workspace,
            None,
            undoable,
            vec![worktrees::remove(&root, &path, force)],
        )
    }

    // ---------------------------------------------------------------- views

    pub(super) fn worktrees_view<'a>(
        &'a self,
        repo: &'a OpenRepo,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let state = &repo.worktrees;
        let Some(list) = &state.list else {
            return text(t!("workspaces.loading")).color(p.muted).into();
        };
        let enabled = self.idle();
        let missing = list.iter().filter(|t| t.missing).count();
        let mut header = row![
            column![
                text(t!("workspaces.title"))
                    .size(18)
                    .font(sans(Weight::Semibold)),
                text(t!("workspaces.intro")).size(13).color(p.text_2),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if missing > 0 {
            header = header.push(small_button(
                t!("workspaces.prune").into_owned(),
                enabled.then_some(Message::Worktrees(Msg::Prune)),
                false,
            ));
        }
        header = header.push(small_button(
            t!("workspaces.new").into_owned(),
            (enabled && state.form.is_none()).then_some(Message::Worktrees(Msg::New(None))),
            true,
        ));
        let mut card = column![container(header).padding([14, 16])];
        if let Some(form) = &state.form {
            card = card
                .push(hline(style::separator))
                .push(self.workspace_form(repo, form, p));
        }
        for tree in list {
            card = card
                .push(hline(style::separator))
                .push(self.workspace_row(repo, tree, p));
            if state.locking.as_ref() == Some(&tree.path) {
                card = card.push(self.lock_form(&state.lock_reason, p));
            }
            if state.confirm_force.as_ref() == Some(&tree.path) {
                card = card.push(self.force_confirm(tree, p));
            }
        }
        card = card.push(hline(style::separator)).push(
            container(text(t!("workspaces.drag_hint")).size(12).color(p.muted)).padding([10, 16]),
        );
        let page = container(card)
            .width(Length::Fill)
            .max_width(1080)
            .style(style::card);
        scrollable(container(page).padding(Padding::default().right(12)))
            .style(style::scroll)
            .height(Length::Fill)
            .into()
    }

    fn workspace_row<'a>(
        &'a self,
        repo: &'a OpenRepo,
        tree: &'a Worktree,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let enabled = self.idle();
        let here = tree.path == repo.paths.root;
        let path = tree.path.clone();

        let mut name = row![
            icon(
                Icon::Folder,
                16.0,
                if here { p.accent.strong } else { p.text_2 }
            ),
            text(folder_name(&tree.path))
                .font(mono(Weight::Medium))
                .size(13),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if tree.main {
            name = name.push(
                container(text(t!("workspaces.main")).size(11))
                    .padding([1, 6])
                    .style(style::outlined_badge(gray(p))),
            );
        }
        let folder = column![name, text(display_path(&tree.path)).size(12).color(p.muted),]
            .spacing(2)
            .width(Length::FillPortion(12));

        let on: Element<'a, Message> = match &tree.branch {
            Some(branch) => {
                let tint = style::branch_tint(p, branch);
                container(
                    text(branch.clone())
                        .size(12)
                        .font(sans(Weight::Semibold))
                        .color(tint.text),
                )
                .padding([2, 8])
                .style(style::badge(tint.soft, tint.text))
                .into()
            }
            None if tree.bare => text(t!("workspaces.bare")).size(12).color(p.muted).into(),
            None => text(t!(
                "workspaces.detached",
                commit = tree.head.as_deref().map(short).unwrap_or_default()
            ))
            .size(12)
            .color(p.warning.text)
            .into(),
        };

        let (label, tint) = if tree.missing {
            (t!("workspaces.missing").into_owned(), p.removed)
        } else if tree.changes > 0 {
            (
                t!("workspaces.changes", count = tree.changes).into_owned(),
                p.accent,
            )
        } else {
            (t!("workspaces.clean").into_owned(), p.added)
        };
        let mut state = column![
            container(text(label).size(12).font(sans(Weight::Semibold)))
                .padding([2, 8])
                .style(style::badge(tint.soft, tint.text)),
        ]
        .spacing(4);
        if let Some(reason) = &tree.locked {
            state = state.push(
                row![
                    icon(Icon::Lock, 12.0, p.warning.text),
                    text(if reason.is_empty() {
                        t!("workspaces.locked").into_owned()
                    } else {
                        t!("workspaces.locked_reason", reason = reason).into_owned()
                    })
                    .size(12)
                    .color(p.warning.text),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            );
        }
        if here {
            state = state.push(text(t!("workspaces.here")).size(12).color(p.accent.text));
        }
        if tree.submodules && !tree.main && !tree.missing {
            state = state.push(
                button(text(t!("workspaces.download_submodules")).size(12))
                    .padding(0)
                    .on_press_maybe(
                        enabled.then(|| Message::Worktrees(Msg::DownloadSubmodules(path.clone()))),
                    )
                    .style(style::link),
            );
        }

        let mut actions = row![].spacing(6).align_y(Alignment::Center);
        if !here && !tree.missing && !tree.bare {
            actions = actions.push(small_button(
                t!("workspaces.open").into_owned(),
                enabled.then(|| Message::Worktrees(Msg::Open(path.clone()))),
                true,
            ));
        }
        if !tree.main {
            actions = actions.push(match tree.locked {
                Some(_) => small_button(
                    t!("workspaces.unlock").into_owned(),
                    enabled.then(|| Message::Worktrees(Msg::Unlock(path.clone()))),
                    false,
                ),
                None => small_button(
                    t!("workspaces.lock").into_owned(),
                    enabled.then(|| Message::Worktrees(Msg::StartLock(path.clone()))),
                    false,
                ),
            });
        }
        if !tree.main && !here && !tree.missing && tree.locked.is_none() {
            // Git removes a workspace with downloaded submodules only when
            // forced: advanced mode offers that, with a confirmation.
            let plain = tree.changes == 0 && !(tree.submodules && self.advanced);
            actions = actions.push(if plain {
                small_button(
                    t!("workspaces.remove").into_owned(),
                    enabled.then(|| Message::Worktrees(Msg::Remove(path.clone()))),
                    false,
                )
            } else if self.advanced {
                button(centered(
                    text(t!("workspaces.force_remove"))
                        .size(13)
                        .wrapping(text::Wrapping::None),
                ))
                .padding([0, 10])
                .height(30)
                .on_press_maybe(enabled.then(|| Message::Worktrees(Msg::ForceRemove(path.clone()))))
                .style(style::danger)
                .into()
            } else {
                tooltip(
                    small_button(t!("workspaces.remove").into_owned(), None, false),
                    container(text(t!("workspaces.remove_refused")).size(12))
                        .padding([6, 10])
                        .max_width(300)
                        .style(style::popup),
                    tooltip::Position::Top,
                )
                .into()
            });
        }

        container(
            row![
                folder,
                container(on).width(Length::FillPortion(6)),
                container(state).width(Length::FillPortion(6)),
                container(actions)
                    .width(Length::FillPortion(9))
                    .align_x(iced::alignment::Horizontal::Right),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([12, 16])
        .style(move |_| {
            if here {
                container::Style {
                    background: Some(p.accent.soft.scale_alpha(0.35).into()),
                    ..container::Style::default()
                }
            } else {
                container::Style::default()
            }
        })
        .into()
    }

    fn lock_form<'a>(&'a self, reason: &'a str, p: &'static Palette) -> Element<'a, Message> {
        container(
            column![
                text(t!("workspaces.lock_intro")).size(13).color(p.text_2),
                row![
                    text_input(&t!("workspaces.lock_placeholder"), reason)
                        .id(LOCK_INPUT)
                        .on_input(|v| Message::Worktrees(Msg::LockReason(v)))
                        .on_submit(Message::Worktrees(Msg::Lock))
                        .size(13)
                        .padding([8, 10])
                        .style(style::input),
                    small_button(
                        t!("common.cancel").into_owned(),
                        Some(Message::Worktrees(Msg::CancelLock)),
                        false,
                    ),
                    small_button(
                        t!("workspaces.lock").into_owned(),
                        self.idle().then_some(Message::Worktrees(Msg::Lock)),
                        true,
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(8),
        )
        .padding(Padding::from([10, 16]).left(40))
        .into()
    }

    fn force_confirm<'a>(
        &'a self,
        tree: &'a Worktree,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        container(
            row![
                text(if tree.changes > 0 {
                    t!(
                        "workspaces.force_intro",
                        count = tree.changes,
                        folder = folder_name(&tree.path)
                    )
                } else {
                    t!(
                        "workspaces.force_intro_submodules",
                        folder = folder_name(&tree.path)
                    )
                })
                .size(13)
                .color(p.removed.text)
                .width(Length::Fill),
                small_button(
                    t!("common.cancel").into_owned(),
                    Some(Message::Worktrees(Msg::CancelForce)),
                    false,
                ),
                button(centered(
                    text(t!("workspaces.force_button"))
                        .size(13)
                        .font(sans(Weight::Semibold))
                ))
                .padding([0, 10])
                .height(30)
                .on_press_maybe(self.idle().then_some(Message::Worktrees(Msg::ConfirmForce)))
                .style(style::danger),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([10, 16]).left(40))
        .style(move |_| container::Style {
            background: Some(p.removed.soft.scale_alpha(0.45).into()),
            ..container::Style::default()
        })
        .into()
    }

    fn workspace_form<'a>(
        &'a self,
        repo: &'a OpenRepo,
        form: &'a Form,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let label = |key: &str| text(tr(key)).size(12).color(p.muted);
        let mut kinds = row![].height(32);
        for (kind, key) in [
            (Kind::Branch, "workspaces.kind_branch"),
            (Kind::NewBranch, "workspaces.kind_new_branch"),
            (Kind::Commit, "workspaces.kind_commit"),
        ] {
            kinds = kinds.push(
                button(centered(text(tr(key)).size(13)))
                    .padding([0, 12])
                    .height(Length::Fill)
                    .on_press(Message::Worktrees(Msg::Kind(kind)))
                    .style(style::segment(form.kind == kind)),
            );
        }

        let start: Element<'a, Message> = match form.kind {
            Kind::Branch => {
                // Local branches, then server branches not here yet.
                let mut branches: Vec<String> = repo
                    .branches
                    .iter()
                    .filter(|b| !b.remote)
                    .map(|b| b.name.clone())
                    .collect();
                branches.extend(
                    repo.branches
                        .iter()
                        .filter(|b| {
                            b.remote
                                && !b.name.ends_with("/HEAD")
                                && !branches.contains(&super::history::local_name(repo, &b.name))
                        })
                        .map(|b| b.name.clone())
                        .collect::<Vec<_>>(),
                );
                column![
                    label("workspaces.branch"),
                    pick_list(branches, form.branch.clone(), |b| Message::Worktrees(
                        Msg::Branch(b)
                    ))
                    .placeholder(t!("workspaces.branch_placeholder"))
                    .style(style::pick)
                    .menu_style(style::menu)
                    .text_size(13)
                    .padding([8, 10])
                    .width(Length::Fill),
                ]
                .spacing(4)
                .into()
            }
            Kind::NewBranch => row![
                column![
                    label("workspaces.new_name"),
                    text_input("fix/…", &form.name)
                        .id(NAME_INPUT)
                        .on_input(|v| Message::Worktrees(Msg::Name(v)))
                        .on_submit(Message::Worktrees(Msg::Create))
                        .size(13)
                        .padding([8, 10])
                        .style(style::input),
                ]
                .spacing(4)
                .width(Length::Fill),
                column![
                    label("workspaces.starts_at"),
                    text_input("main", &form.at)
                        .on_input(|v| Message::Worktrees(Msg::At(v)))
                        .on_submit(Message::Worktrees(Msg::Create))
                        .size(13)
                        .padding([8, 10])
                        .style(style::input),
                ]
                .spacing(4)
                .width(Length::Fill),
            ]
            .spacing(12)
            .into(),
            Kind::Commit => column![
                label("workspaces.commit"),
                text_input("a1b2c3d", &form.at)
                    .id(COMMIT_INPUT)
                    .on_input(|v| Message::Worktrees(Msg::At(v)))
                    .on_submit(Message::Worktrees(Msg::Create))
                    .size(13)
                    .padding([8, 10])
                    .style(style::input),
            ]
            .spacing(4)
            .into(),
        };

        let folder = column![
            label("workspaces.folder"),
            row![
                text_input("", &form.folder)
                    .on_input(|v| Message::Worktrees(Msg::Folder(v)))
                    .on_submit(Message::Worktrees(Msg::Create))
                    .font(style::MONO)
                    .size(13)
                    .padding([8, 10])
                    .style(style::input),
                small_button(
                    t!("workspaces.choose").into_owned(),
                    Some(Message::Worktrees(Msg::PickFolder)),
                    false,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(4);

        let mut body = column![
            text(t!("workspaces.form_intro")).size(13).color(p.text_2),
            container(kinds).padding(1).style(style::segmented_frame),
            start,
            folder,
        ]
        .spacing(10);

        if repo.paths.root.join(".gitmodules").is_file() {
            body = body.push(
                checkbox(form.submodules)
                    .label(t!("workspaces.with_submodules"))
                    .size(16)
                    .spacing(8)
                    .text_size(13)
                    .on_toggle(|on| Message::Worktrees(Msg::Submodules(on)))
                    .style(style::check),
            );
        }

        // Why it can't be created yet, in plain words.
        let held = match (form.kind, &form.branch) {
            (Kind::Branch, Some(branch)) => repo.worktrees.holding(branch),
            _ => None,
        };
        let folder_path = PathBuf::from(form.folder.trim());
        let occupied = !form.folder.trim().is_empty()
            && std::fs::read_dir(&folder_path).is_ok_and(|mut d| d.next().is_some());
        let start = form.start();
        if let Some(tree) = held {
            let open = tree.path != repo.paths.root;
            let mut line = row![
                icon(Icon::Warning, 14.0, p.warning.text),
                text(t!(
                    "workspaces.already_open",
                    branch = form.branch.clone().unwrap_or_default(),
                    folder = folder_name(&tree.path)
                ))
                .size(13)
                .color(p.warning.text)
                .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if open {
                line = line.push(small_button(
                    t!("workspaces.open_that").into_owned(),
                    self.idle()
                        .then(|| Message::Worktrees(Msg::Open(tree.path.clone()))),
                    false,
                ));
            }
            body = body.push(line);
        } else if occupied {
            body = body.push(
                text(t!("workspaces.folder_not_empty"))
                    .size(13)
                    .color(p.warning.text),
            );
        } else if form.kind == Kind::NewBranch
            && !form.name.trim().is_empty()
            && !crate::git::remotes::valid_name(form.name.trim())
        {
            body = body.push(
                text(t!("workspaces.bad_name"))
                    .size(13)
                    .color(p.warning.text),
            );
        }

        let ready = self.idle()
            && held.is_none()
            && !occupied
            && start.is_some()
            && !folder_path.as_os_str().is_empty();
        let command = start
            .as_ref()
            .map(|s| {
                format!(
                    "$ {}",
                    worktrees::add(&repo.paths.root, &folder_path, s).display()
                )
            })
            .unwrap_or_default();
        body = body.push(
            row![
                text(command)
                    .font(style::MONO)
                    .size(12)
                    .color(p.text_2)
                    .width(Length::Fill),
                small_button(
                    t!("common.cancel").into_owned(),
                    Some(Message::Worktrees(Msg::CloseForm)),
                    false,
                ),
                small_button(
                    t!("workspaces.create").into_owned(),
                    ready.then_some(Message::Worktrees(Msg::Create)),
                    true,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        container(body).padding([12, 16]).into()
    }
}

/// A long folder keeps its end, the part that tells workspaces apart.
fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    let chars: Vec<char> = path.chars().collect();
    if chars.len() > 44 {
        format!("…{}", chars[chars.len() - 43..].iter().collect::<String>())
    } else {
        path.into_owned()
    }
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

fn gray(p: &'static Palette) -> style::Tint {
    style::Tint {
        strong: p.muted,
        soft: p.border_soft,
        text: p.text_2,
    }
}
