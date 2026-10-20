//! The "Submodules" and "Locked files" screens: the repositories included in
//! this one, and the Git LFS locks that keep two people from editing the same
//! binary file.

use iced::widget::{column, row};

use super::undo::Undoable;
use super::*;
use crate::git::lfs::{self, Age, Lock};
use crate::git::submodules::{self, State, Submodule};
use crate::git::templates;
use crate::git::undo::{FileChange, Inverse, Moves};

/// What the two screens show, read in the background.
#[derive(Debug, Clone, Default)]
pub struct Data {
    pub submodules: Vec<Submodule>,
    pub lfs: Option<Lfs>,
}

/// Git LFS in this repository (`None` in [`Data`]: LFS is not installed).
#[derive(Debug, Clone, Default)]
pub struct Lfs {
    /// The hooks that download and lock files are installed here.
    pub hooks: bool,
    pub patterns: Vec<String>,
    pub files: Vec<String>,
    /// The server's locks; `None` until asked (it needs the network).
    pub locks: Option<Result<Vec<Lock>, String>>,
    /// `user.name`, to recognise our locks when the server can't tell.
    pub user: Option<String>,
}

#[derive(Default)]
pub struct ModulesState {
    pub data: Option<Data>,
    /// The servers are being asked what is new.
    pub checking: bool,
    /// The servers were asked since the screen was opened.
    pub checked: bool,
    /// The submodule whose details are open.
    pub expanded: Option<String>,
    pub adding: bool,
    pub add_url: String,
    pub add_path: String,
    pub track_draft: String,
}

impl ModulesState {
    /// How many submodules need a look (the sidebar badge).
    pub fn attention(&self) -> usize {
        self.data.as_ref().map_or(0, |d| {
            d.submodules.iter().filter(|s| s.needs_attention()).count()
        })
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Loaded(PathBuf, Box<Data>, Vec<CommandRecord>, Reach),
    Check,
    Expand(String),
    Open(String),
    Download(String),
    Update(String),
    Reset(String),
    Record(String),
    SyncAll,
    ToggleAdd,
    AddUrl(String),
    AddPath(String),
    ConfirmAdd,
    Lock(String),
    Unlock(String),
    ForceUnlock(String),
    CopyName(String),
    TrackDraft(String),
    Track,
    InstallHooks,
    Template(&'static str),
    BackToParent,
}

/// How far a reading of the screens goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Only this computer.
    Local,
    /// Also the LFS server's locks (after locking or releasing a file).
    Locks,
    /// Also what is new on each submodule's server, and the locks.
    All,
}

/// Reads the submodules and LFS state. With `network`, asks the servers
/// first (what is new in each submodule, and the locks).
async fn load(root: PathBuf, git_dir: PathBuf, reach: Reach) -> (Data, Vec<CommandRecord>) {
    let mut records = Vec::new();
    let mut data = Data::default();
    if root.join(".gitmodules").exists() {
        let config = run(submodules::config_command(&root)).await;
        let status = run(submodules::status_command(&root)).await;
        data.submodules = submodules::parse(&config.stdout, &status.stdout);
        records.extend([config, status]);
        if reach == Reach::All {
            for sub in data.submodules.iter().filter(|s| s.downloaded()) {
                records.push(run(submodules::fetch(&root, &sub.path).background()).await);
            }
        }
        records.extend(submodules::inspect(&root, &mut data.submodules).await);
    }
    let version = run(lfs::version_command(&root)).await;
    let installed = version.success();
    records.push(version);
    if installed {
        let tracked = run(lfs::tracked_command(&root)).await;
        let files = run(lfs::files_command(&root)).await;
        let user = run(GitCommand::new(["config", "--get", "user.name"])
            .in_dir(&root)
            .background())
        .await;
        let hooks = std::fs::read_to_string(git_dir.join("hooks").join("pre-push"))
            .is_ok_and(|h| h.contains("git-lfs") || h.contains("git lfs"));
        let mut info = Lfs {
            hooks,
            patterns: lfs::parse_tracked(&tracked.stdout),
            files: files.stdout.lines().map(str::to_string).collect(),
            locks: None,
            user: user
                .success()
                .then(|| user.stdout.trim().to_string())
                .filter(|u| !u.is_empty()),
        };
        records.extend([tracked, files, user]);
        if reach != Reach::Local && !info.patterns.is_empty() {
            let mut verified = run(lfs::locks_command(&root, true)).await;
            if !verified.success() {
                // Servers without lock verification still list the locks.
                records.push(verified);
                verified = run(lfs::locks_command(&root, false)).await;
            }
            info.locks = Some(if verified.success() {
                lfs::parse_locks(&verified.stdout)
            } else {
                Err(verified.error_summary())
            });
            records.push(verified);
        }
        data.lfs = Some(info);
    }
    (data, records)
}

impl App {
    /// Reads the submodules and LFS state of the open repository.
    pub(super) fn load_modules(&mut self, reach: Reach) -> Task<Message> {
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        if reach == Reach::All {
            repo.modules.checking = true;
        }
        let root = repo.paths.root.clone();
        let git_dir = repo.paths.git_dir.clone();
        Task::perform(
            load(root.clone(), git_dir, reach),
            move |(data, records)| {
                Message::Modules(Msg::Loaded(root.clone(), Box::new(data), records, reach))
            },
        )
    }

    pub(super) fn modules_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Loaded(root, data, records, reach) => {
                for record in records {
                    self.log(record);
                }
                let Some(repo) = self.repo.as_mut().filter(|r| r.paths.root == root) else {
                    return Task::none();
                };
                let state = &mut repo.modules;
                let mut data = *data;
                // A local reading keeps the locks the server gave last time.
                if reach == Reach::Local
                    && let (Some(new), Some(old)) = (
                        data.lfs.as_mut(),
                        state.data.as_ref().and_then(|d| d.lfs.as_ref()),
                    )
                {
                    new.locks = old.locks.clone();
                }
                if state.expanded.is_none() {
                    state.expanded = data
                        .submodules
                        .iter()
                        .find(|s| s.behind.is_some_and(|n| n > 0))
                        .map(|s| s.path.clone());
                }
                state.data = Some(data);
                if reach == Reach::All {
                    state.checking = false;
                    state.checked = true;
                    Task::none()
                } else if !state.checked
                    && !state.checking
                    && matches!(repo.section, Section::Submodules | Section::Locks)
                {
                    self.load_modules(Reach::All)
                } else {
                    Task::none()
                }
            }
            Msg::Check => self.load_modules(Reach::All),
            Msg::Expand(path) => {
                if let Some(repo) = &mut self.repo {
                    let state = &mut repo.modules;
                    state.expanded = if state.expanded.as_deref() == Some(&path) {
                        None
                    } else {
                        Some(path)
                    };
                }
                Task::none()
            }
            Msg::Open(path) => {
                let Some(repo) = &self.repo else {
                    return Task::none();
                };
                let parent = repo.paths.root.clone();
                self.pending_parent = Some(parent.clone());
                self.open(submodules::folder(&parent, &path))
            }
            Msg::BackToParent => match self.repo.as_ref().and_then(|r| r.parent.clone()) {
                Some(parent) => {
                    self.pending_section = Some(Section::Submodules);
                    self.open(parent)
                }
                None => Task::none(),
            },
            Msg::Download(path) => {
                let Some(root) = self.root() else {
                    return Task::none();
                };
                let mut undoable = Undoable::new(
                    t!("undo.submodule_download", path = path).into_owned(),
                    Moves::Index,
                );
                undoable.inverse = Some(Inverse {
                    undo: vec![
                        GitCommand::new(["submodule", "deinit", "--force", "--", &path])
                            .in_dir(&root),
                    ],
                    redo: vec![submodules::download(&root, Some(&path))],
                });
                self.perform(
                    Action::Submodule,
                    Some(t!("busy.submodule_download", path = path).into_owned()),
                    Some(undoable),
                    vec![submodules::download(&root, Some(&path))],
                )
            }
            Msg::Update(path) => {
                let Some(root) = self.root() else {
                    return Task::none();
                };
                let mut undoable = Undoable::new(
                    t!("undo.submodule_update", path = path).into_owned(),
                    Moves::Index,
                );
                // The index goes back or forth, then the submodule follows it.
                let follow =
                    || vec![GitCommand::new(["submodule", "update", "--", &path]).in_dir(&root)];
                undoable.inverse = Some(Inverse {
                    undo: follow(),
                    redo: follow(),
                });
                self.perform(
                    Action::Submodule,
                    Some(t!("busy.submodule_update", path = path).into_owned()),
                    Some(undoable),
                    submodules::update_to_server(&root, &path),
                )
            }
            Msg::Reset(path) => {
                let Some(repo) = &self.repo else {
                    return Task::none();
                };
                let root = repo.paths.root.clone();
                let Some(current) = repo
                    .modules
                    .data
                    .as_ref()
                    .and_then(|d| d.submodules.iter().find(|s| s.path == path))
                    .and_then(|s| s.checked_out.clone())
                else {
                    return Task::none();
                };
                let mut undoable = Undoable::new(
                    t!("undo.submodule_reset", path = path).into_owned(),
                    Moves::Index,
                );
                undoable.inverse = Some(Inverse {
                    undo: vec![
                        GitCommand::new(["checkout", "--quiet", &current]).in_dir(root.join(&path)),
                    ],
                    redo: vec![submodules::reset(&root, &path)],
                });
                self.perform(
                    Action::Submodule,
                    None,
                    Some(undoable),
                    vec![submodules::reset(&root, &path)],
                )
            }
            Msg::Record(path) => {
                let Some(root) = self.root() else {
                    return Task::none();
                };
                let undoable = Undoable::new(
                    t!("undo.submodule_record", path = path).into_owned(),
                    Moves::Index,
                );
                self.perform(
                    Action::Submodule,
                    None,
                    Some(undoable),
                    vec![GitCommand::new(["add", "--", &path]).in_dir(&root)],
                )
            }
            Msg::SyncAll => {
                let Some(root) = self.root() else {
                    return Task::none();
                };
                self.perform(
                    Action::Submodule,
                    Some(t!("busy.submodule_sync").into_owned()),
                    None,
                    submodules::sync_all(&root),
                )
            }
            Msg::ToggleAdd => {
                if let Some(repo) = &mut self.repo {
                    repo.modules.adding = !repo.modules.adding;
                }
                Task::none()
            }
            Msg::AddUrl(url) => {
                if let Some(repo) = &mut self.repo {
                    let state = &mut repo.modules;
                    // The folder follows the address until it is typed.
                    let suggested = submodules::folder_from_url(&state.add_url);
                    if state.add_path.is_empty() || state.add_path == suggested {
                        state.add_path = submodules::folder_from_url(&url);
                    }
                    state.add_url = url;
                }
                Task::none()
            }
            Msg::AddPath(path) => {
                if let Some(repo) = &mut self.repo {
                    repo.modules.add_path = path;
                }
                Task::none()
            }
            Msg::ConfirmAdd => {
                let Some(repo) = &mut self.repo else {
                    return Task::none();
                };
                let url = repo.modules.add_url.trim().to_string();
                let path = repo.modules.add_path.trim().trim_matches('/').to_string();
                if url.is_empty() || path.is_empty() {
                    return Task::none();
                }
                repo.modules.adding = false;
                repo.modules.add_url.clear();
                repo.modules.add_path.clear();
                let root = repo.paths.root.clone();
                let mut undoable = Undoable::new(
                    t!("undo.submodule_add", path = path).into_owned(),
                    Moves::Index,
                );
                undoable.watch_file = Some(root.join(".gitmodules"));
                self.perform(
                    Action::Submodule,
                    Some(t!("busy.submodule_add", url = url).into_owned()),
                    Some(undoable),
                    vec![submodules::add(&root, &url, &path)],
                )
            }
            Msg::Lock(path) | Msg::Unlock(path) | Msg::ForceUnlock(path)
                if self.root().is_none() || path.is_empty() =>
            {
                Task::none()
            }
            Msg::Lock(path) => {
                let root = self.root().unwrap_or_default();
                let mut undoable =
                    Undoable::new(t!("undo.lock", path = path).into_owned(), Moves::Index);
                undoable.inverse = Some(Inverse {
                    undo: vec![lfs::unlock(&root, &path, false)],
                    redo: vec![lfs::lock(&root, &path)],
                });
                self.perform(
                    Action::Lock,
                    Some(t!("busy.locking", path = path).into_owned()),
                    Some(undoable),
                    vec![lfs::lock(&root, &path)],
                )
            }
            Msg::Unlock(path) => {
                let root = self.root().unwrap_or_default();
                let mut undoable =
                    Undoable::new(t!("undo.unlock", path = path).into_owned(), Moves::Index);
                undoable.inverse = Some(Inverse {
                    undo: vec![lfs::lock(&root, &path)],
                    redo: vec![lfs::unlock(&root, &path, false)],
                });
                self.perform(
                    Action::Lock,
                    Some(t!("busy.unlocking", path = path).into_owned()),
                    Some(undoable),
                    vec![lfs::unlock(&root, &path, false)],
                )
            }
            Msg::ForceUnlock(path) => {
                let root = self.root().unwrap_or_default();
                // Someone else's lock can't be given back: not undoable.
                self.perform(
                    Action::Lock,
                    Some(t!("busy.unlocking", path = path).into_owned()),
                    None,
                    vec![lfs::unlock(&root, &path, true)],
                )
            }
            Msg::CopyName(name) => {
                self.notice = Some(t!("locks.name_copied", name = name).into_owned());
                iced::clipboard::write(name)
            }
            Msg::TrackDraft(draft) => {
                if let Some(repo) = &mut self.repo {
                    repo.modules.track_draft = draft;
                }
                Task::none()
            }
            Msg::Track => {
                let Some(repo) = &mut self.repo else {
                    return Task::none();
                };
                let mut pattern = repo.modules.track_draft.trim().to_string();
                if pattern.is_empty() {
                    return Task::none();
                }
                // `pdf` or `.pdf` means every PDF.
                if !pattern.contains(['*', '/', '?', '[']) {
                    pattern = format!("*.{}", pattern.trim_start_matches('.'));
                }
                repo.modules.track_draft.clear();
                let root = repo.paths.root.clone();
                let mut undoable = Undoable::new(
                    t!("undo.track", pattern = pattern).into_owned(),
                    Moves::Index,
                );
                undoable.watch_file = Some(root.join(".gitattributes"));
                self.perform(
                    Action::Lock,
                    None,
                    Some(undoable),
                    vec![lfs::track(&root, &pattern)],
                )
            }
            Msg::InstallHooks => {
                let Some(root) = self.root() else {
                    return Task::none();
                };
                self.perform(Action::Lock, None, None, vec![lfs::install(&root)])
            }
            Msg::Template(id) => self.apply_template(id),
        }
    }

    fn root(&self) -> Option<PathBuf> {
        self.repo.as_ref().map(|r| r.paths.root.clone())
    }

    /// Appends a `.gitignore` template's missing rules (undoable).
    fn apply_template(&mut self, id: &'static str) -> Task<Message> {
        let Some(template) = templates::find(id) else {
            return Task::none();
        };
        let Some(root) = self.root() else {
            return Task::none();
        };
        let file = root.join(".gitignore");
        let before = std::fs::read(&file).ok();
        let content = before
            .as_deref()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        let Some(updated) = templates::append(&content, template) else {
            self.notice = Some(t!("templates.already", name = template.name).into_owned());
            return Task::none();
        };
        let shell = format!(
            "cat >> .gitignore <<'EOF'\n{}EOF",
            updated[content.len()..].trim_start_matches('\n')
        );
        let result = std::fs::write(&file, &updated).map_err(|e| e.to_string());
        let ok = result.is_ok();
        let record = CommandRecord::local(shell, &root, result);
        if !ok {
            self.error = Some(record.error_summary());
            self.log(record);
            return Task::none();
        }
        self.log(record);
        let mut undoable = Undoable::new(
            t!("undo.template", name = template.name).into_owned(),
            Moves::Index,
        );
        undoable.file = Some(FileChange {
            path: file.clone(),
            before,
            after: std::fs::read(&file).ok(),
        });
        self.notice = Some(t!("templates.added", name = template.name).into_owned());
        self.perform(Action::Ignore, None, Some(undoable), Vec::new())
    }

    /// Shown in a repository opened from its parent's submodule list.
    pub(super) fn parent_banner<'a>(&'a self, parent: &'a Path) -> Element<'a, Message> {
        container(
            row![
                text(t!("submodules.inside", parent = folder_name(parent))).width(Length::Fill),
                button(centered(
                    text(t!("submodules.back", parent = folder_name(parent))).size(13)
                ))
                .padding([0, 12])
                .height(32)
                .on_press_maybe(self.idle().then_some(Message::Modules(Msg::BackToParent)))
                .style(style::secondary),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .width(Length::Fill)
        .style(style::banner_info)
        .into()
    }

    /// The submodules screen: the submodules, and the locks next to them.
    pub(super) fn submodules_view<'a>(
        &'a self,
        repo: &'a OpenRepo,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let state = &repo.modules;
        let Some(data) = &state.data else {
            return text(t!("submodules.loading")).color(p.muted).into();
        };
        let card = self.submodules_card(repo, data, p);
        let locks = self.locks_card(repo, data, true, p);
        let page: Element<'a, Message> = if self.window.width >= 1440.0 {
            row![
                container(card).width(Length::FillPortion(8)),
                container(locks)
                    .width(Length::FillPortion(4))
                    .max_width(420),
            ]
            .spacing(16)
            .into()
        } else {
            column![card, locks].spacing(16).into()
        };
        scrollable(container(page).padding(Padding::default().right(12)))
            .style(style::scroll)
            .height(Length::Fill)
            .into()
    }

    pub(super) fn locks_view<'a>(
        &'a self,
        repo: &'a OpenRepo,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let Some(data) = &repo.modules.data else {
            return text(t!("submodules.loading")).color(p.muted).into();
        };
        let mut page = column![self.locks_card(repo, data, false, p)]
            .spacing(16)
            .max_width(880);
        if data.lfs.is_some() {
            page = page.push(self.tracking_card(repo, data, p));
        }
        scrollable(page.padding(Padding::default().right(12)))
            .style(style::scroll)
            .height(Length::Fill)
            .into()
    }

    fn submodules_card<'a>(
        &'a self,
        repo: &'a OpenRepo,
        data: &'a Data,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let state = &repo.modules;
        let enabled = self.idle();
        let header = row![
            column![
                text(t!("submodules.title"))
                    .size(18)
                    .font(sans(Weight::Semibold)),
                text(t!("submodules.intro")).size(13).color(p.text_2),
            ]
            .spacing(2)
            .width(Length::Fill),
            small_button(
                t!("submodules.sync_all").into_owned(),
                (enabled && !data.submodules.is_empty()).then_some(Message::Modules(Msg::SyncAll)),
                false,
            ),
            small_button(
                t!("submodules.add").into_owned(),
                enabled.then_some(Message::Modules(Msg::ToggleAdd)),
                true,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let mut card = column![container(header).padding([14, 16])];

        if state.adding {
            card = card
                .push(hline(style::separator))
                .push(self.add_form(repo, p));
        }

        if data.submodules.is_empty() {
            card = card.push(hline(style::separator)).push(
                container(text(t!("submodules.none")).size(13).color(p.muted)).padding([16, 16]),
            );
            return container(card)
                .width(Length::Fill)
                .style(style::card)
                .into();
        }

        let heading = |key: &str| {
            text(tr(key).to_uppercase())
                .size(11)
                .font(sans(Weight::Semibold))
                .color(p.muted)
        };
        card = card.push(hline(style::separator)).push(
            container(
                row![
                    heading("submodules.col_folder").width(Length::FillPortion(13)),
                    heading("submodules.col_state").width(Length::FillPortion(14)),
                    heading("submodules.col_commit").width(96),
                    Space::new().width(ACTION_WIDTH),
                ]
                .spacing(12),
            )
            .padding([8, 16]),
        );
        for sub in &data.submodules {
            card = card.push(hline(style::separator));
            let expanded = state.expanded.as_deref() == Some(&sub.path);
            let line = self.submodule_row(sub, expanded, p);
            if expanded && sub.behind.is_some_and(|n| n > 0) {
                card = card.push(
                    container(column![line, self.update_details(repo, sub, p)]).style(move |_| {
                        container::Style {
                            background: Some(p.warning.soft.scale_alpha(0.45).into()),
                            ..container::Style::default()
                        }
                    }),
                );
            } else {
                card = card.push(line);
            }
        }
        let footer = if state.checking {
            t!("submodules.checking").into_owned()
        } else {
            t!("submodules.checked_hint").into_owned()
        };
        card = card.push(hline(style::separator)).push(
            container(
                row![
                    text(footer).size(12).color(p.muted).width(Length::Fill),
                    button(text(t!("submodules.check")).size(12))
                        .padding(0)
                        .on_press_maybe(
                            (enabled && !state.checking).then_some(Message::Modules(Msg::Check))
                        )
                        .style(style::link),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([10, 16]),
        );
        container(card)
            .width(Length::Fill)
            .style(style::card)
            .into()
    }

    fn add_form<'a>(&'a self, repo: &'a OpenRepo, p: &'static Palette) -> Element<'a, Message> {
        let state = &repo.modules;
        let ready =
            self.idle() && !state.add_url.trim().is_empty() && !state.add_path.trim().is_empty();
        let command = submodules::add(
            &repo.paths.root,
            state.add_url.trim(),
            state.add_path.trim(),
        )
        .display();
        container(
            column![
                text(t!("submodules.add_intro")).size(13).color(p.text_2),
                row![
                    column![
                        text(t!("submodules.add_url")).size(12).color(p.muted),
                        text_input(&t!("submodules.add_url_placeholder"), &state.add_url)
                            .on_input(|v| Message::Modules(Msg::AddUrl(v)))
                            .on_submit(Message::Modules(Msg::ConfirmAdd))
                            .size(13)
                            .padding([8, 10])
                            .style(style::input),
                    ]
                    .spacing(4)
                    .width(Length::FillPortion(3)),
                    column![
                        text(t!("submodules.add_path")).size(12).color(p.muted),
                        text_input("lib/…", &state.add_path)
                            .on_input(|v| Message::Modules(Msg::AddPath(v)))
                            .on_submit(Message::Modules(Msg::ConfirmAdd))
                            .size(13)
                            .padding([8, 10])
                            .style(style::input),
                    ]
                    .spacing(4)
                    .width(Length::FillPortion(2)),
                ]
                .spacing(12),
                row![
                    text(format!("$ {command}"))
                        .font(style::MONO)
                        .size(12)
                        .color(p.text_2)
                        .width(Length::Fill),
                    small_button(
                        t!("common.cancel").into_owned(),
                        Some(Message::Modules(Msg::ToggleAdd)),
                        false
                    ),
                    small_button(
                        t!("submodules.add_button").into_owned(),
                        ready.then_some(Message::Modules(Msg::ConfirmAdd)),
                        true
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(10),
        )
        .padding([12, 16])
        .into()
    }

    fn submodule_row<'a>(
        &'a self,
        sub: &'a Submodule,
        expanded: bool,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let enabled = self.idle();
        let path = sub.path.clone();
        let behind = sub.behind.unwrap_or(0);
        let (label, tint) = match sub.state {
            State::NotDownloaded => (t!("submodules.not_downloaded").into_owned(), gray(p)),
            State::Conflict => (t!("submodules.conflict").into_owned(), p.removed),
            _ if sub.dirty => (t!("submodules.dirty").into_owned(), p.accent),
            State::Moved => (t!("submodules.moved").into_owned(), p.warning),
            _ if behind > 0 => (
                t!("submodules.behind", count = behind).into_owned(),
                p.warning,
            ),
            _ => (t!("submodules.up_to_date").into_owned(), p.added),
        };
        let chip = container(text(label).size(12).font(sans(Weight::Semibold)))
            .padding([2, 8])
            .style(style::badge(tint.soft, tint.text));
        let mut state_cell = column![chip].spacing(4);
        if behind > 0 && sub.downloaded() {
            state_cell = state_cell.push(
                button(
                    text(if expanded {
                        t!("submodules.hide_details")
                    } else {
                        t!("submodules.details")
                    })
                    .size(12),
                )
                .padding(0)
                .on_press(Message::Modules(Msg::Expand(path.clone())))
                .style(style::link),
            );
        }
        let open = || {
            small_button(
                t!("submodules.open").into_owned(),
                enabled.then(|| Message::Modules(Msg::Open(path.clone()))),
                false,
            )
        };
        let actions: Element<'a, Message> = match sub.state {
            State::NotDownloaded => small_button(
                t!("submodules.download").into_owned(),
                enabled.then(|| Message::Modules(Msg::Download(path.clone()))),
                false,
            ),
            State::Conflict => small_button(
                t!("submodules.to_changes").into_owned(),
                Some(Message::SelectSection(Section::Changes)),
                false,
            ),
            _ if sub.dirty => small_button(
                t!("submodules.see_changes").into_owned(),
                enabled.then(|| Message::Modules(Msg::Open(path.clone()))),
                false,
            ),
            State::Moved => row![
                small_button(
                    t!("submodules.reset").into_owned(),
                    enabled.then(|| Message::Modules(Msg::Reset(path.clone()))),
                    false,
                ),
                small_button(
                    t!("submodules.record").into_owned(),
                    enabled.then(|| Message::Modules(Msg::Record(path.clone()))),
                    true,
                ),
            ]
            .spacing(6)
            .into(),
            _ if behind > 0 => row![
                open(),
                small_button(
                    t!("submodules.update").into_owned(),
                    enabled.then(|| Message::Modules(Msg::Update(path.clone()))),
                    true,
                ),
            ]
            .spacing(6)
            .into(),
            _ => open(),
        };
        let commit = match (sub.state, &sub.checked_out) {
            (State::Moved, Some(now)) => format!("{} → {}", short(&sub.pinned), short(now)),
            _ => short(&sub.pinned).to_string(),
        };
        container(
            row![
                column![
                    text(sub.path.clone()).font(mono(Weight::Medium)).size(13),
                    text(display_url(&sub.url)).size(12).color(p.muted),
                ]
                .width(Length::FillPortion(13)),
                container(state_cell).width(Length::FillPortion(14)),
                text(commit)
                    .font(style::MONO)
                    .size(12)
                    .color(p.text_2)
                    .width(96),
                container(actions)
                    .width(ACTION_WIDTH)
                    .align_x(iced::alignment::Horizontal::Right),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([12, 16])
        .into()
    }

    /// What "update" does, for a submodule behind its server.
    fn update_details<'a>(
        &'a self,
        repo: &'a OpenRepo,
        sub: &'a Submodule,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let quoted = |hash: &str, subject: Option<&str>| match subject {
            Some(s) => {
                t!("submodules.commit_subject", hash = short(hash), subject = s).into_owned()
            }
            None => short(hash).to_string(),
        };
        let pinned = quoted(&sub.pinned, sub.pinned_subject.as_deref());
        let latest = match &sub.latest {
            Some((hash, subject)) => format!(
                "{} (+{})",
                quoted(hash, Some(subject)),
                sub.behind.unwrap_or(0)
            ),
            None => format!("+{}", sub.behind.unwrap_or(0)),
        };
        let commands = submodules::update_to_server(&repo.paths.root, &sub.path)
            .iter()
            .map(GitCommand::display)
            .collect::<Vec<_>>()
            .join(" && ");
        let line = |label: String, value: String| {
            row![
                text(label).size(13).color(p.muted).width(180),
                text(value).font(style::MONO).size(13).width(Length::Fill),
            ]
            .spacing(12)
        };
        container(
            container(
                column![
                    line(
                        t!("submodules.pinned_by", repo = folder_name(&repo.paths.root))
                            .into_owned(),
                        pinned
                    ),
                    line(t!("submodules.latest_on_server").into_owned(), latest),
                    text(t!(
                        "submodules.update_explained",
                        repo = folder_name(&repo.paths.root)
                    ))
                    .size(13)
                    .color(p.text_2),
                    text(format!("$ {commands}"))
                        .font(style::MONO)
                        .size(12)
                        .color(p.text_2),
                ]
                .spacing(8),
            )
            .padding([12, 14])
            .width(Length::Fill)
            .style(style::outlined_box(p.warning.strong)),
        )
        .padding(Padding::default().left(16).right(16).bottom(14))
        .into()
    }

    /// The locked files. `compact`: next to the submodules, only the files
    /// with a lock and the lockable ones.
    fn locks_card<'a>(
        &'a self,
        repo: &'a OpenRepo,
        data: &'a Data,
        compact: bool,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let enabled = self.idle();
        let mut card = column![
            container(
                column![
                    text(t!("locks.title"))
                        .size(18)
                        .font(sans(Weight::Semibold)),
                    text(t!("locks.intro")).size(13).color(p.text_2),
                ]
                .spacing(2)
            )
            .padding([14, 16]),
        ];
        let note = |message: String| -> Element<'a, Message> {
            container(text(message).size(13).color(p.muted))
                .padding([14, 16])
                .into()
        };
        let Some(info) = &data.lfs else {
            card = card
                .push(hline(style::separator))
                .push(note(t!("locks.no_lfs").into_owned()));
            return container(card)
                .width(Length::Fill)
                .style(style::card)
                .into();
        };
        if !info.hooks {
            card = card.push(hline(style::separator)).push(
                container(
                    row![
                        text(t!("locks.no_hooks"))
                            .size(13)
                            .color(p.warning.text)
                            .width(Length::Fill),
                        small_button(
                            t!("locks.install_hooks").into_owned(),
                            enabled.then_some(Message::Modules(Msg::InstallHooks)),
                            false
                        ),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding([10, 16])
                .style(style::tinted(p.warning)),
            );
        }
        if info.patterns.is_empty() {
            card = card
                .push(hline(style::separator))
                .push(note(t!("locks.no_patterns").into_owned()));
        } else {
            let locks: &[Lock] = match &info.locks {
                Some(Ok(locks)) => locks,
                _ => &[],
            };
            match &info.locks {
                None => {
                    card = card
                        .push(hline(style::separator))
                        .push(note(t!("locks.asking").into_owned()));
                }
                Some(Err(e)) => {
                    card = card
                        .push(hline(style::separator))
                        .push(note(t!("locks.server_error", detail = e).into_owned()));
                }
                Some(Ok(_)) => {}
            }
            // Locked files first, then the free ones.
            let mut paths: Vec<&str> = locks.iter().map(|l| l.path.as_str()).collect();
            for file in &info.files {
                if !paths.contains(&file.as_str()) {
                    paths.push(file);
                }
            }
            let limit = if compact { 6 } else { usize::MAX };
            let hidden = paths.len().saturating_sub(limit);
            if paths.is_empty() && info.locks.as_ref().is_some_and(Result::is_ok) {
                card = card
                    .push(hline(style::separator))
                    .push(note(t!("locks.no_files").into_owned()));
            }
            for path in paths.into_iter().take(limit) {
                let lock = locks.iter().find(|l| l.path == path);
                card = card
                    .push(hline(style::separator))
                    .push(self.lock_row(path, lock, info, p));
            }
            if hidden > 0 {
                card = card.push(hline(style::separator)).push(
                    container(
                        button(text(t!("locks.more", count = hidden)).size(12))
                            .padding(0)
                            .on_press(Message::SelectSection(Section::Locks))
                            .style(style::link),
                    )
                    .padding([10, 16]),
                );
            }
        }
        // A file nobody holds, as the example of locking.
        let held = |f: &String| matches!(&info.locks, Some(Ok(locks)) if locks.iter().any(|l| &l.path == f));
        let example = info
            .files
            .iter()
            .find(|f| !held(f))
            .map(|f| lfs::lock(&repo.paths.root, f).display())
            .unwrap_or_else(|| "git lfs lock <file>".to_string());
        let patterns = if info.patterns.is_empty() {
            "—".to_string()
        } else {
            info.patterns.join("  ")
        };
        card = card.push(hline(style::separator)).push(
            container(
                column![
                    row![
                        text(t!("locks.tracked")).size(12).color(p.text_2),
                        text(patterns).font(style::MONO).size(12).color(p.text_2),
                    ]
                    .spacing(6)
                    .wrap(),
                    text(format!("$ {example}"))
                        .font(style::MONO)
                        .size(12)
                        .color(p.text_2),
                ]
                .spacing(8),
            )
            .padding([12, 16]),
        );
        container(card)
            .width(Length::Fill)
            .style(style::card)
            .into()
    }

    fn lock_row<'a>(
        &'a self,
        path: &'a str,
        lock: Option<&'a Lock>,
        info: &'a Lfs,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let enabled = self.idle() && info.locks.as_ref().is_some_and(Result::is_ok);
        let file = path.to_string();
        let (line, color, actions, tint): (String, Color, Element<'a, Message>, Option<Color>) =
            match lock {
                None => (
                    t!("locks.free").into_owned(),
                    p.muted,
                    small_button(
                        t!("locks.lock").into_owned(),
                        enabled.then(|| Message::Modules(Msg::Lock(file.clone()))),
                        true,
                    ),
                    None,
                ),
                Some(lock) => {
                    let ours = lock
                        .ours
                        .unwrap_or_else(|| info.user.as_deref() == Some(lock.owner.as_str()));
                    let since = lock.locked_at.map(|at| match lfs::age(at, lfs::now()) {
                        Age::Minutes(n) => t!("locks.minutes", count = n).into_owned(),
                        Age::Hours(n) => t!("locks.hours", count = n).into_owned(),
                        Age::Days(n) => t!("locks.days", count = n).into_owned(),
                    });
                    if ours {
                        let mut line = t!("locks.by_you").into_owned();
                        if let Some(since) = since {
                            line = format!("{line} · {since}");
                        }
                        (
                            line,
                            p.accent.text,
                            small_button(
                                t!("locks.unlock").into_owned(),
                                enabled.then(|| Message::Modules(Msg::Unlock(file.clone()))),
                                false,
                            ),
                            None,
                        )
                    } else {
                        let mut line = t!("locks.by", owner = lock.owner).into_owned();
                        if let Some(since) = since {
                            line = format!("{line} · {since}");
                        }
                        line = format!("{line} · {}", t!("locks.read_only"));
                        let mut buttons = row![small_button(
                            t!("locks.copy_name").into_owned(),
                            Some(Message::Modules(Msg::CopyName(lock.owner.clone()))),
                            false,
                        )]
                        .spacing(6);
                        if self.advanced {
                            buttons = buttons.push(small_button(
                                t!("locks.force_unlock").into_owned(),
                                enabled.then(|| Message::Modules(Msg::ForceUnlock(file.clone()))),
                                false,
                            ));
                        }
                        (line, p.removed.text, buttons.into(), Some(p.removed.soft))
                    }
                }
            };
        let row = container(
            row![
                column![
                    text(path.to_string()).font(style::MONO).size(13),
                    text(line).size(12).color(color),
                ]
                .spacing(2)
                .width(Length::Fill),
                actions,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([12, 16])
        .width(Length::Fill);
        match tint {
            Some(background) => row.style(move |_| container::Style {
                background: Some(background.scale_alpha(0.35).into()),
                ..container::Style::default()
            }),
            None => row,
        }
        .into()
    }

    /// Which files go to Git LFS (Locks screen).
    fn tracking_card<'a>(
        &'a self,
        repo: &'a OpenRepo,
        data: &'a Data,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let state = &repo.modules;
        let patterns = data
            .lfs
            .as_ref()
            .map(|l| l.patterns.as_slice())
            .unwrap_or(&[]);
        let draft = state.track_draft.trim();
        let preview = if draft.is_empty() {
            lfs::track(&repo.paths.root, "*.kicad_pcb").display()
        } else if draft.contains(['*', '/', '?', '[']) {
            lfs::track(&repo.paths.root, draft).display()
        } else {
            lfs::track(
                &repo.paths.root,
                &format!("*.{}", draft.trim_start_matches('.')),
            )
            .display()
        };
        let mut list = row![].spacing(6);
        for pattern in patterns {
            list = list.push(
                container(text(pattern.clone()).font(style::MONO).size(12))
                    .padding([2, 8])
                    .style(style::badge(p.accent.soft, p.accent.text)),
            );
        }
        container(
            column![
                text(t!("lfs.title")).size(18).font(sans(Weight::Semibold)),
                text(t!("lfs.intro")).size(13).color(p.text_2),
                list.wrap(),
                row![
                    text_input(&t!("lfs.placeholder"), &state.track_draft)
                        .on_input(|v| Message::Modules(Msg::TrackDraft(v)))
                        .on_submit(Message::Modules(Msg::Track))
                        .size(13)
                        .padding([8, 10])
                        .width(220)
                        .style(style::input),
                    small_button(
                        t!("lfs.track").into_owned(),
                        (self.idle() && !draft.is_empty()).then_some(Message::Modules(Msg::Track)),
                        true
                    ),
                    text(format!("$ {preview}"))
                        .font(style::MONO)
                        .size(12)
                        .color(p.muted),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                text(t!("lfs.commit_hint")).size(12).color(p.muted),
            ]
            .spacing(10),
        )
        .padding([14, 16])
        .width(Length::Fill)
        .style(style::card)
        .into()
    }

    /// The `.gitignore` templates card of the Config screen.
    pub(super) fn templates_card<'a>(
        &'a self,
        repo: &'a OpenRepo,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let content =
            std::fs::read_to_string(repo.paths.root.join(".gitignore")).unwrap_or_default();
        let mut list = row![].spacing(8);
        for template in templates::TEMPLATES {
            let applied = templates::applied(&content, template);
            let label = if applied {
                format!("✓ {}", template.name)
            } else {
                template.name.to_string()
            };
            list = list.push(tooltip(
                button(centered(text(label).size(12)))
                    .padding([0, 10])
                    .height(30)
                    .on_press_maybe(
                        (self.idle() && !applied)
                            .then_some(Message::Modules(Msg::Template(template.id))),
                    )
                    .style(style::secondary),
                container(text(template.lines.join("\n")).font(style::MONO).size(12))
                    .padding([6, 10])
                    .style(style::card),
                tooltip::Position::Bottom,
            ));
        }
        container(
            column![
                text(t!("templates.title"))
                    .size(15)
                    .font(sans(Weight::Semibold)),
                text(t!("templates.intro")).size(13).color(p.text_2),
                list.wrap(),
            ]
            .spacing(10),
        )
        .padding([12, 16])
        .width(Length::Fill)
        .style(style::card)
        .into()
    }
}

/// Width of the action column of the submodules table.
const ACTION_WIDTH: f32 = 220.0;

fn gray(p: &'static Palette) -> style::Tint {
    style::Tint {
        strong: p.muted,
        soft: p.border_soft,
        text: p.text_2,
    }
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

/// `https://github.com/labo/x.git` → `github.com/labo/x`.
fn display_url(url: &str) -> String {
    let url = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("ssh://")
        .trim_start_matches("git@");
    let url = url
        .strip_suffix(".git")
        .unwrap_or(url)
        .replacen(':', "/", 1);
    // Long local paths keep their end, the part that tells them apart.
    let chars: Vec<char> = url.chars().collect();
    if chars.len() > 40 {
        format!("…{}", chars[chars.len() - 39..].iter().collect::<String>())
    } else {
        url
    }
}

fn small_button<'a>(
    label: String,
    message: Option<Message>,
    primary: bool,
) -> Element<'a, Message> {
    let mut content = text(label).size(13).wrapping(text::Wrapping::None);
    if primary {
        content = content.font(sans(Weight::Semibold));
    }
    button(centered(content))
        .padding([0, 10])
        .height(30)
        .on_press_maybe(message)
        .style(if primary {
            style::primary
        } else {
            style::secondary
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_urls() {
        assert_eq!(
            display_url("https://github.com/labo-fpga/axi-uart-lite.git"),
            "github.com/labo-fpga/axi-uart-lite"
        );
        assert_eq!(
            display_url("git@github.com:labo/ddr.git"),
            "github.com/labo/ddr"
        );
        assert_eq!(display_url("../common-vhdl.git"), "../common-vhdl");
        assert_eq!(
            display_url("/tmp/a-very-long-folder-name/with/many/parts/repo.git"),
            "…y-long-folder-name/with/many/parts/repo"
        );
    }
}
