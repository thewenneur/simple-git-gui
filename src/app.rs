//! The application state, its messages and its views.
//!
//! The layout follows the approved wireframes: a top bar, a sidebar with the
//! sections and branches, the current section, and the dark Git journal at
//! the bottom.

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::font::Weight;
use iced::theme::Mode;
use iced::widget::{
    Space, button, checkbox, column, container, row, scrollable, space, text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Length, Padding, Subscription, Task, Theme};

use crate::git::repo::{self, Branch, Commit, Operation, RepoPaths};
use crate::git::runner::{CommandRecord, GitCommand, run};
use crate::git::status::{EntryKind, FileEntry, Status};
use crate::i18n::{self, Lang, tr};
use crate::ui::icons::{Icon, icon};
use crate::ui::style::{self, Palette, journal as jc, mono, sans};

/// How many commits the history shows.
const LOG_LIMIT: usize = 200;
/// How many journal entries are kept.
const JOURNAL_LIMIT: usize = 500;
/// How often the open repository is re-read in the background.
const AUTO_REFRESH: Duration = Duration::from_secs(4);
/// Height of the journal when expanded.
const JOURNAL_HEIGHT: f32 = 210.0;
const SIDEBAR_WIDTH: f32 = 240.0;

#[derive(Default)]
pub struct App {
    lang: Option<Lang>,
    /// Dark theme, following the system unless `SIMPLE_GIT_THEME` is set.
    dark: bool,
    theme_forced: bool,
    git_version: Option<String>,
    repo: Option<OpenRepo>,
    /// Every git command run so far, newest last.
    journal: Vec<CommandRecord>,
    journal_collapsed: bool,
    /// Shows command outputs, background commands and the git console.
    advanced: bool,
    show_background: bool,
    /// A user-visible action is running; most buttons are disabled meanwhile.
    busy: Option<String>,
    error: Option<String>,
    clone_url: String,
    clone_parent: Option<PathBuf>,
    console_input: String,
}

struct OpenRepo {
    paths: RepoPaths,
    status: Option<Status>,
    operation: Option<Operation>,
    commits: Vec<Commit>,
    branches: Vec<Branch>,
    section: Section,
    /// A background refresh is running; avoids piling them up.
    refreshing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Changes,
    History,
}

#[derive(Debug, Clone)]
pub enum Message {
    GitChecked(CommandRecord),
    SystemTheme(Mode),
    PickRepoFolder,
    RepoFolderPicked(Option<PathBuf>),
    Opened(Result<RepoPaths, String>, CommandRecord),
    Tick,
    /// Status, log and branch list, in that order.
    Refreshed(PathBuf, Box<[CommandRecord; 3]>),
    SelectSection(Section),
    CloneUrlChanged(String),
    PickCloneParent,
    CloneParentPicked(Option<PathBuf>),
    StartClone,
    Cloned(CommandRecord, PathBuf),
    ToggleAdvanced(bool),
    ToggleBackground(bool),
    ToggleJournal,
    CopyJournal,
    ConsoleChanged(String),
    ConsoleSubmit,
    ConsoleDone(CommandRecord),
    ClearJournal,
    SetLanguage(Lang),
    DismissError,
}

impl App {
    /// Starts the app, opening `initial_dir` right away when given
    /// (e.g. `simple-git-gui path/to/repo`).
    pub fn new(initial_dir: Option<PathBuf>) -> (Self, Task<Message>) {
        let lang = Lang::from_system();
        i18n::set(lang);
        let forced = std::env::var("SIMPLE_GIT_THEME").ok();
        let mut app = Self {
            lang: Some(lang),
            dark: forced.as_deref() == Some("dark"),
            theme_forced: matches!(forced.as_deref(), Some("dark" | "light")),
            ..Self::default()
        };
        let check = Task::perform(run(GitCommand::new(["--version"])), Message::GitChecked);
        let theme = iced::system::theme().map(Message::SystemTheme);
        let open = match initial_dir {
            Some(dir) => app.open(dir),
            None => Task::none(),
        };
        (app, Task::batch([check, theme, open]))
    }

    pub fn title(&self) -> String {
        match &self.repo {
            Some(repo) => t!("app.title_repo", name = folder_name(&repo.paths.root)).into_owned(),
            None => "Simple Git".to_string(),
        }
    }

    pub fn theme(&self) -> Theme {
        style::theme(self.dark)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let refresh = if self.repo.is_some() && self.busy.is_none() {
            iced::time::every(AUTO_REFRESH).map(|_| Message::Tick)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            refresh,
            iced::system::theme_changes().map(Message::SystemTheme),
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::GitChecked(record) => {
                if record.success() {
                    self.git_version = Some(record.stdout.trim().to_string());
                } else {
                    self.error =
                        Some(t!("error.git_missing", detail = record.error_summary()).into_owned());
                }
                self.log(record);
                Task::none()
            }
            Message::SystemTheme(mode) => {
                if !self.theme_forced {
                    self.dark = mode == Mode::Dark;
                }
                Task::none()
            }
            Message::PickRepoFolder => Task::perform(
                pick_folder(t!("dialog.open_repo").into_owned()),
                Message::RepoFolderPicked,
            ),
            Message::RepoFolderPicked(None) => Task::none(),
            Message::RepoFolderPicked(Some(dir)) => self.open(dir),
            Message::Opened(result, record) => {
                self.busy = None;
                self.log(record);
                match result {
                    Ok(paths) => {
                        self.error = None;
                        self.repo = Some(OpenRepo {
                            paths,
                            status: None,
                            operation: None,
                            commits: Vec::new(),
                            branches: Vec::new(),
                            section: Section::Changes,
                            refreshing: false,
                        });
                        self.refresh(false)
                    }
                    Err(e) => {
                        self.error = Some(e);
                        Task::none()
                    }
                }
            }
            Message::Tick => self.refresh(true),
            Message::Refreshed(root, records) => {
                let [status, log, branches] = *records;
                let Some(repo) = self.repo.as_mut().filter(|r| r.paths.root == root) else {
                    return Task::none();
                };
                repo.refreshing = false;
                repo.operation = repo::operation_in_progress(&repo.paths.git_dir);
                match repo::parse_status(&status) {
                    Ok(s) => repo.status = Some(s),
                    Err(e) => self.error = Some(e),
                }
                // An empty repository has no history: that is not an error.
                repo.commits = if log.success() {
                    repo::parse_log(&log.stdout)
                } else {
                    Vec::new()
                };
                repo.branches = repo::parse_branches(&branches.stdout);
                self.log(status);
                self.log(log);
                self.log(branches);
                Task::none()
            }
            Message::SelectSection(section) => {
                if let Some(repo) = &mut self.repo {
                    repo.section = section;
                }
                Task::none()
            }
            Message::CloneUrlChanged(url) => {
                self.clone_url = url;
                Task::none()
            }
            Message::PickCloneParent => Task::perform(
                pick_folder(t!("dialog.clone_parent").into_owned()),
                Message::CloneParentPicked,
            ),
            Message::CloneParentPicked(dir) => {
                if dir.is_some() {
                    self.clone_parent = dir;
                }
                Task::none()
            }
            Message::StartClone => {
                let (Some(parent), false) = (&self.clone_parent, self.clone_url.trim().is_empty())
                else {
                    return Task::none();
                };
                let url = self.clone_url.trim().to_string();
                let dest = parent.join(repo::clone_folder_name(&url));
                if dest.exists() {
                    self.error =
                        Some(t!("error.folder_exists", path = dest.display()).into_owned());
                    return Task::none();
                }
                self.busy = Some(t!("busy.cloning", url = url).into_owned());
                self.error = None;
                let command = repo::clone_command(&url, &dest);
                Task::perform(run(command), move |record| {
                    Message::Cloned(record, dest.clone())
                })
            }
            Message::Cloned(record, dest) => {
                self.busy = None;
                let ok = record.success();
                if !ok {
                    self.error = Some(
                        t!("error.clone_failed", detail = record.error_summary()).into_owned(),
                    );
                }
                self.log(record);
                if ok {
                    self.clone_url.clear();
                    self.open(dest)
                } else {
                    Task::none()
                }
            }
            Message::ToggleAdvanced(on) => {
                self.advanced = on;
                Task::none()
            }
            Message::ToggleBackground(on) => {
                self.show_background = on;
                Task::none()
            }
            Message::ToggleJournal => {
                self.journal_collapsed = !self.journal_collapsed;
                Task::none()
            }
            Message::CopyJournal => {
                let lines: Vec<String> = self
                    .visible_journal()
                    .map(|r| format!("$ {}", r.command.display()))
                    .collect();
                iced::clipboard::write(lines.join("\n"))
            }
            Message::ConsoleChanged(input) => {
                self.console_input = input;
                Task::none()
            }
            Message::ConsoleSubmit => {
                let args = match parse_console(&self.console_input) {
                    Ok(args) if !args.is_empty() => args,
                    Ok(_) => return Task::none(),
                    Err(e) => {
                        self.error = Some(e);
                        return Task::none();
                    }
                };
                let mut command = GitCommand::new(args).network();
                if let Some(repo) = &self.repo {
                    command = command.in_dir(&repo.paths.root);
                }
                self.busy = Some(t!("busy.running", command = command.display()).into_owned());
                self.console_input.clear();
                Task::perform(run(command), Message::ConsoleDone)
            }
            Message::ConsoleDone(record) => {
                self.busy = None;
                if !record.success() {
                    self.error = Some(record.error_summary());
                }
                self.log(record);
                self.refresh(false)
            }
            Message::ClearJournal => {
                self.journal.clear();
                Task::none()
            }
            Message::SetLanguage(lang) => {
                i18n::set(lang);
                self.lang = Some(lang);
                Task::none()
            }
            Message::DismissError => {
                self.error = None;
                Task::none()
            }
        }
    }

    fn log(&mut self, record: CommandRecord) {
        self.journal.push(record);
        if self.journal.len() > JOURNAL_LIMIT {
            let excess = self.journal.len() - JOURNAL_LIMIT;
            self.journal.drain(..excess);
        }
    }

    fn visible_journal(&self) -> impl Iterator<Item = &CommandRecord> {
        self.journal
            .iter()
            .filter(|r| self.show_background || !r.command.background)
    }

    fn open(&mut self, dir: PathBuf) -> Task<Message> {
        self.busy = Some(t!("busy.opening").into_owned());
        Task::perform(
            async move { repo::discover(&dir).await },
            |(result, record)| Message::Opened(result, record),
        )
    }

    fn refresh(&mut self, background: bool) -> Task<Message> {
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        if repo.refreshing {
            return Task::none();
        }
        repo.refreshing = true;
        let root = repo.paths.root.clone();
        let [status, log, branches] = [
            repo::status_command(&root),
            repo::log_command(&root, LOG_LIMIT),
            repo::branches_command(&root),
        ]
        .map(|c| if background { c.background() } else { c });
        Task::perform(
            async move {
                let (status, log, branches) = tokio::join!(run(status), run(log), run(branches));
                Message::Refreshed(root, Box::new([status, log, branches]))
            },
            |message| message,
        )
    }

    fn idle(&self) -> bool {
        self.busy.is_none()
    }

    fn palette(&self) -> &'static Palette {
        if self.dark {
            &style::DARK
        } else {
            &style::LIGHT
        }
    }

    // ---------------------------------------------------------------- views

    pub fn view(&self) -> Element<'_, Message> {
        let p = self.palette();

        let mut content = column![].spacing(12).padding(16).height(Length::Fill);
        if let Some(error) = &self.error {
            content = content.push(
                container(
                    row![
                        text(error).width(Length::Fill),
                        button(text(t!("common.ok")).font(sans(Weight::Semibold)))
                            .on_press(Message::DismissError)
                            .style(style::link),
                    ]
                    .align_y(Alignment::Center)
                    .spacing(10),
                )
                .padding([8, 12])
                .width(Length::Fill)
                .style(style::banner_error),
            );
        }
        if let Some(busy) = &self.busy {
            content = content.push(
                container(text(busy))
                    .padding([8, 12])
                    .width(Length::Fill)
                    .style(style::banner_info),
            );
        }
        let main: Element<'_, Message> = match &self.repo {
            None => self.welcome_view(p),
            Some(repo) => self.section_view(repo, p),
        };
        content = content.push(main);

        let body: Element<'_, Message> = match &self.repo {
            Some(repo) => row![
                self.sidebar(repo, p),
                vline(),
                column![content, self.journal_view()].width(Length::Fill)
            ]
            .height(Length::Fill)
            .into(),
            None => column![content, self.journal_view()]
                .height(Length::Fill)
                .into(),
        };

        container(column![
            self.top_bar(p),
            hline(style::strong_separator),
            body
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::page)
        .into()
    }

    fn top_bar(&self, p: &'static Palette) -> Element<'_, Message> {
        let logo = row![
            icon(Icon::Logo, 22.0, p.accent.strong),
            text("Simple Git").size(16).font(sans(Weight::Semibold)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let mut bar = row![logo].spacing(12).align_y(Alignment::Center);

        if let Some(repo) = &self.repo {
            let repo_button = button(centered(
                row![
                    icon(Icon::Folder, 16.0, p.text_2),
                    text(folder_name(&repo.paths.root)).font(sans(Weight::Medium)),
                    text(short_path(&repo.paths.root, 40))
                        .size(12)
                        .color(p.muted),
                    icon(Icon::ChevronDown, 12.0, p.text_2),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ))
            .padding([0, 12])
            .height(36)
            .on_press_maybe(self.idle().then_some(Message::PickRepoFolder))
            .style(style::repo_pill);
            bar = bar.push(tooltip(
                repo_button,
                container(text(t!("toolbar.change_repo")).size(12))
                    .padding([4, 8])
                    .style(style::card),
                tooltip::Position::Bottom,
            ));
            if let Some(status) = &repo.status {
                bar = bar.push(branch_pill(status, p));
            }
        }

        bar = bar.push(space::horizontal());

        let mut langs = row![].height(36);
        for lang in Lang::ALL {
            langs = langs.push(
                button(centered(text(lang.label()).size(13).font(sans(
                    if self.lang == Some(lang) {
                        Weight::Semibold
                    } else {
                        Weight::Normal
                    },
                ))))
                .padding([0, 10])
                .height(Length::Fill)
                .on_press(Message::SetLanguage(lang))
                .style(style::segment(self.lang == Some(lang))),
            );
        }
        bar = bar.push(container(langs).padding(1).style(style::segmented_frame));
        bar = bar.push(
            checkbox(self.advanced)
                .label(t!("toolbar.advanced"))
                .size(16)
                .spacing(8)
                .on_toggle(Message::ToggleAdvanced)
                .style(style::check),
        );

        container(bar)
            .padding([10, 16])
            .width(Length::Fill)
            .style(style::header)
            .into()
    }

    fn sidebar<'a>(&'a self, repo: &'a OpenRepo, p: &'static Palette) -> Element<'a, Message> {
        let changed = repo.status.as_ref().map_or(0, |s| {
            s.entries
                .iter()
                .filter(|e| e.has_unstaged() || e.has_staged())
                .count()
        });

        let mut nav = column![
            nav_item(
                Icon::Changes,
                t!("nav.changes").into_owned(),
                (changed > 0).then_some(changed),
                Section::Changes,
                repo.section,
                p,
            ),
            nav_item(
                Icon::History,
                t!("nav.history").into_owned(),
                None,
                Section::History,
                repo.section,
                p,
            ),
        ]
        .spacing(4);

        let local: Vec<_> = repo.branches.iter().filter(|b| !b.remote).collect();
        let remote: Vec<_> = repo.branches.iter().filter(|b| b.remote).collect();

        if !local.is_empty() {
            nav = nav
                .push(Space::new().height(16))
                .push(caption(t!("sidebar.my_branches").into_owned(), p));
            for branch in local {
                nav = nav.push(branch_row(branch, p));
            }
        }
        if !remote.is_empty() {
            nav = nav
                .push(Space::new().height(10))
                .push(caption(t!("sidebar.remote").into_owned(), p));
            for branch in remote {
                nav = nav.push(
                    container(text(branch.name.clone()).size(13).color(p.text_2))
                        .padding([0, 12])
                        .height(28)
                        .align_y(Alignment::Center),
                );
            }
        }

        container(scrollable(nav).style(style::scroll))
            .padding([16, 12])
            .width(SIDEBAR_WIDTH)
            .height(Length::Fill)
            .style(style::sidebar)
            .into()
    }

    fn welcome_view(&self, p: &'static Palette) -> Element<'_, Message> {
        let open = column![
            text(t!("welcome.open_title"))
                .size(15)
                .font(sans(Weight::Semibold)),
            text(t!("welcome.open_hint")).color(p.text_2),
            button(text(t!("welcome.pick_folder")).font(sans(Weight::Semibold)))
                .padding([10, 16])
                .on_press_maybe(self.idle().then_some(Message::PickRepoFolder))
                .style(style::primary),
        ]
        .spacing(10);

        let parent = match &self.clone_parent {
            Some(path) => path.display().to_string(),
            None => t!("welcome.no_folder").into_owned(),
        };
        let can_clone =
            self.idle() && self.clone_parent.is_some() && !self.clone_url.trim().is_empty();
        let destination = match (&self.clone_parent, self.clone_url.trim()) {
            (Some(path), url) if !url.is_empty() => t!(
                "welcome.clone_destination",
                path = path.join(repo::clone_folder_name(url)).display()
            )
            .into_owned(),
            _ => String::new(),
        };
        let clone = column![
            text(t!("welcome.clone_title"))
                .size(15)
                .font(sans(Weight::Semibold)),
            text(t!("welcome.clone_hint")).color(p.text_2),
            text_input(&t!("welcome.clone_placeholder"), &self.clone_url)
                .font(style::MONO)
                .size(13)
                .padding([8, 10])
                .on_input(Message::CloneUrlChanged)
                .on_submit_maybe(can_clone.then_some(Message::StartClone))
                .style(style::input),
            row![
                button(text(t!("welcome.pick_parent")))
                    .padding([8, 12])
                    .on_press_maybe(self.idle().then_some(Message::PickCloneParent))
                    .style(style::secondary),
                text(parent).size(13).color(p.muted),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            (!destination.is_empty()).then(|| text(destination).size(12).color(p.muted)),
            button(text(t!("welcome.clone_button")).font(sans(Weight::Semibold)))
                .padding([10, 16])
                .on_press_maybe(can_clone.then_some(Message::StartClone))
                .style(style::primary),
        ]
        .spacing(10);

        let version = text(
            self.git_version
                .clone()
                .unwrap_or_else(|| t!("welcome.git_searching").into_owned()),
        )
        .font(style::MONO)
        .size(11)
        .color(p.muted);

        scrollable(
            column![
                container(open)
                    .padding(20)
                    .width(Length::Fill)
                    .style(style::card),
                container(clone)
                    .padding(20)
                    .width(Length::Fill)
                    .style(style::card),
                version,
            ]
            .spacing(16)
            .max_width(720),
        )
        .style(style::scroll)
        .height(Length::Fill)
        .into()
    }

    fn section_view<'a>(&'a self, repo: &'a OpenRepo, p: &'static Palette) -> Element<'a, Message> {
        let mut view = column![].spacing(12).height(Length::Fill);
        if let Some(op) = repo.operation {
            view = view.push(
                container(text(tr(op.key())).font(sans(Weight::Medium)))
                    .padding([8, 12])
                    .width(Length::Fill)
                    .style(style::banner_warning),
            );
        }
        let section = match repo.section {
            Section::Changes => match &repo.status {
                Some(status) => self.changes_view(status, p),
                None => text(t!("repo.reading_status")).color(p.muted).into(),
            },
            Section::History => self.history_view(&repo.commits, p),
        };
        view.push(section).into()
    }

    fn changes_view<'a>(&'a self, status: &'a Status, p: &'static Palette) -> Element<'a, Message> {
        let conflicts: Vec<_> = status.conflicts().collect();
        let staged: Vec<_> = status.entries.iter().filter(|e| e.has_staged()).collect();
        let unstaged: Vec<_> = status
            .entries
            .iter()
            .filter(|e| {
                matches!(e.kind, EntryKind::Tracked | EntryKind::Untracked) && e.has_unstaged()
            })
            .collect();

        let header = row![
            text(t!("changes.title"))
                .size(15)
                .font(sans(Weight::Semibold))
        ]
        .align_y(Alignment::Center);

        let mut list = column![];
        if conflicts.is_empty() && staged.is_empty() && unstaged.is_empty() {
            list = list.push(container(text(t!("changes.none")).color(p.muted)).padding([16, 14]));
        }
        for (key, color, entries) in [
            ("changes.conflicts", p.removed.strong, conflicts),
            ("changes.staged", p.added.strong, staged),
            ("changes.unstaged", p.text_2, unstaged),
        ] {
            if entries.is_empty() {
                continue;
            }
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
            for entry in entries {
                list = list.push(self.file_row(entry, p));
            }
        }

        container(column![
            container(header).padding([12, 14]),
            hline(style::separator),
            scrollable(list.padding(Padding::default().bottom(8)))
                .style(style::scroll)
                .height(Length::Fill),
        ])
        .max_width(760)
        .height(Length::Fill)
        .style(style::card)
        .into()
    }

    fn file_row<'a>(&'a self, entry: &'a FileEntry, p: &'static Palette) -> Element<'a, Message> {
        let (dir, name) = split_path(&entry.path);
        let state = lowercase_first(&tr(entry.state_key()));
        let subtitle = match (&entry.orig_path, dir) {
            (Some(orig), _) => format!("{orig} → · {state}"),
            (None, Some(dir)) => format!("{dir}/ · {state}"),
            (None, None) => state,
        };

        let mut line = row![
            column![
                text(name.to_string()).size(13).font(mono(Weight::Medium)),
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
        }
        if self.advanced {
            line = line.push(
                text(entry.xy.clone())
                    .size(12)
                    .font(style::MONO)
                    .color(p.muted),
            );
        }
        container(line).padding([8, 14]).into()
    }

    fn history_view<'a>(
        &'a self,
        commits: &'a [Commit],
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let header = container(
            text(t!("history.title"))
                .size(15)
                .font(sans(Weight::Semibold)),
        )
        .padding([12, 14]);

        let mut list = column![];
        if commits.is_empty() {
            list = list.push(container(text(t!("history.none")).color(p.muted)).padding([16, 14]));
        }
        for commit in commits {
            let mut line = row![
                text(commit.short.clone())
                    .size(12)
                    .font(style::MONO)
                    .color(p.muted)
                    .width(70)
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            for r in &commit.refs {
                let tint = style::branch_tint(p, r.strip_prefix("tag: ").unwrap_or(r));
                line = line.push(
                    container(text(r.clone()).size(11).font(sans(Weight::Semibold)))
                        .padding([1, 7])
                        .style(style::badge(tint.soft, tint.text)),
                );
            }
            line = line
                .push(text(commit.subject.clone()).size(13).width(Length::Fill))
                .push(text(commit.author.clone()).size(12).color(p.text_2))
                .push(
                    text(commit.date.clone())
                        .size(12)
                        .font(style::MONO)
                        .color(p.muted),
                );
            list = list.push(container(line).padding([7, 14]));
        }

        container(column![
            header,
            hline(style::separator),
            scrollable(list).style(style::scroll).height(Length::Fill),
        ])
        .height(Length::Fill)
        .style(style::card)
        .into()
    }

    fn journal_view(&self) -> Element<'_, Message> {
        let mut header = row![
            text(t!("journal.title"))
                .size(13)
                .font(sans(Weight::Semibold))
                .color(jc::TITLE),
            text(t!("journal.hint")).size(12).color(jc::HINT),
            space::horizontal(),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if self.advanced && !self.journal_collapsed {
            header = header.push(
                checkbox(self.show_background)
                    .label(t!("journal.show_background"))
                    .size(14)
                    .text_size(12)
                    .on_toggle(Message::ToggleBackground)
                    .style(style::journal_check),
            );
        }
        if !self.journal_collapsed {
            header = header
                .push(journal_button(
                    t!("journal.copy").into_owned(),
                    Message::CopyJournal,
                ))
                .push(journal_button(
                    t!("journal.clear").into_owned(),
                    Message::ClearJournal,
                ));
        }
        header = header.push(
            button(icon(
                if self.journal_collapsed {
                    Icon::ChevronUp
                } else {
                    Icon::ChevronDown
                },
                12.0,
                jc::TEXT,
            ))
            .width(26)
            .height(26)
            .padding(6)
            .on_press(Message::ToggleJournal)
            .style(style::journal_button),
        );

        let mut panel = column![container(header).padding([8, 16])];

        if !self.journal_collapsed {
            let mut entries = column![].spacing(2);
            for record in self.visible_journal() {
                entries = entries.push(self.journal_entry(record));
            }
            panel = panel.push(hline(style::journal_separator)).push(
                scrollable(entries.padding([8, 16]))
                    .anchor_bottom()
                    .style(style::scroll)
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
            if self.advanced {
                panel = panel.push(
                    container(
                        row![
                            text("$").font(style::MONO).size(12).color(jc::PROMPT),
                            text("git").font(style::MONO).size(12),
                            text_input(&t!("journal.console_placeholder"), &self.console_input)
                                .font(style::MONO)
                                .size(12)
                                .padding([5, 8])
                                .on_input(Message::ConsoleChanged)
                                .on_submit_maybe(self.idle().then_some(Message::ConsoleSubmit))
                                .style(style::journal_input),
                            journal_button(t!("journal.run").into_owned(), Message::ConsoleSubmit),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    )
                    .padding(Padding::from([0, 16]).bottom(10)),
                );
            }
        }

        container(panel)
            .width(Length::Fill)
            .height(if self.journal_collapsed {
                Length::Shrink
            } else {
                Length::Fixed(JOURNAL_HEIGHT)
            })
            .style(style::journal)
            .into()
    }

    fn journal_entry<'a>(&'a self, record: &'a CommandRecord) -> Element<'a, Message> {
        let mark = if record.success() {
            icon(Icon::Check, 12.0, jc::OK)
        } else {
            icon(Icon::Cross, 12.0, jc::FAIL)
        };
        let mut line = row![
            container(mark).height(17).align_y(Alignment::Center),
            text("$").font(style::MONO).size(12).color(jc::PROMPT),
            text(record.command.display())
                .font(style::MONO)
                .size(12)
                .width(Length::Fill),
        ]
        .spacing(10);
        if let Some(dir) = record.command.cwd.as_deref().filter(|_| self.advanced) {
            line = line.push(
                text(folder_name(dir))
                    .font(style::MONO)
                    .size(11)
                    .color(jc::DURATION),
            );
        }
        line = line.push(
            text(format!("{} ms", record.duration.as_millis()))
                .font(style::MONO)
                .size(12)
                .color(jc::DURATION),
        );
        let mut entry = column![line].spacing(2);
        if self.advanced || !record.success() {
            let output = clip(record.stdout.trim_end(), 20);
            let errors = clip(record.stderr.trim_end(), 20);
            let detail = |s: String, color: Color| {
                container(text(s).font(style::MONO).size(11).color(color))
                    .padding(Padding::default().left(42))
            };
            if !output.is_empty() && self.advanced {
                entry = entry.push(detail(output, jc::HINT));
            }
            if !errors.is_empty() {
                entry = entry.push(detail(errors, jc::FAIL));
            }
            if !record.success() && record.stderr.trim().is_empty() {
                entry = entry.push(detail(record.error_summary(), jc::FAIL));
            }
        }
        entry.into()
    }
}

// ------------------------------------------------------------------ pieces

fn hline<'a>(style: fn(&Theme) -> container::Style) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(1)
        .style(style)
        .into()
}

fn vline<'a>() -> Element<'a, Message> {
    container(Space::new())
        .width(1)
        .height(Length::Fill)
        .style(style::strong_separator)
        .into()
}

/// Centers a button's content vertically in its fixed height.
fn centered<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .into()
}

/// Shortens a long path from the left: `…/scratchpad/carte-fpga-uart`.
fn short_path(path: &Path, max_chars: usize) -> String {
    let full = path.display().to_string();
    let count = full.chars().count();
    if count <= max_chars {
        return full;
    }
    let tail: String = full.chars().skip(count - (max_chars - 1)).collect();
    format!("…{tail}")
}

fn caption<'a>(label: String, p: &'static Palette) -> Element<'a, Message> {
    container(
        text(label.to_uppercase())
            .size(11)
            .font(sans(Weight::Semibold))
            .color(p.muted),
    )
    .padding([4, 12])
    .into()
}

fn nav_item<'a>(
    glyph: Icon,
    label: String,
    count: Option<usize>,
    section: Section,
    current: Section,
    p: &'static Palette,
) -> Element<'a, Message> {
    let active = section == current;
    let mut content = row![
        icon(glyph, 18.0, if active { p.accent.strong } else { p.text_2 }),
        text(label).font(sans(if active {
            Weight::Semibold
        } else {
            Weight::Normal
        })),
        space::horizontal(),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if let Some(count) = count {
        content = content.push(
            container(text(count.to_string()).size(12).color(Color::WHITE))
                .padding([0, 7])
                .style(style::badge(p.accent.strong, Color::WHITE)),
        );
    }
    button(centered(content))
        .padding([0, 12])
        .height(40)
        .width(Length::Fill)
        .on_press(Message::SelectSection(section))
        .style(style::nav(active))
        .into()
}

fn branch_row<'a>(branch: &'a Branch, p: &'static Palette) -> Element<'a, Message> {
    let tint = style::branch_tint(p, &branch.name);
    let mut line = row![
        container(Space::new())
            .width(10)
            .height(10)
            .style(style::dot(tint.strong)),
        text(branch.name.clone())
            .size(13)
            .font(sans(if branch.current {
                Weight::Semibold
            } else {
                Weight::Normal
            }))
            .color(if branch.current { tint.text } else { p.text })
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if branch.current {
        line = line.push(text(t!("sidebar.here")).size(12).color(tint.text));
    }
    let row = container(line)
        .padding([0, 12])
        .height(32)
        .align_y(Alignment::Center);
    if branch.current {
        row.style(style::tinted(tint)).into()
    } else {
        row.into()
    }
}

fn branch_pill<'a>(status: &'a Status, p: &'static Palette) -> Element<'a, Message> {
    let branch = &status.branch;
    let (name, tint) = match &branch.head {
        Some(head) => (head.clone(), style::branch_tint(p, head)),
        None => (t!("pill.detached").into_owned(), p.warning),
    };
    let sync = match (&branch.upstream, branch.ahead, branch.behind) {
        (None, _, _) if branch.oid.is_none() => String::new(),
        (None, _, _) => t!("pill.unpublished").into_owned(),
        (Some(_), 0, 0) => t!("pill.up_to_date").into_owned(),
        (Some(_), a, 0) => t!("pill.ahead", count = a).into_owned(),
        (Some(_), 0, b) => t!("pill.behind", count = b).into_owned(),
        (Some(_), a, b) => format!(
            "{} · {}",
            t!("pill.ahead", count = a),
            t!("pill.behind", count = b)
        ),
    };
    button(centered(
        row![
            icon(Icon::BranchSmall, 14.0, tint.strong),
            text(name).font(sans(Weight::Semibold)),
            text(sync).size(12),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ))
    .padding([0, 12])
    .height(36)
    .style(style::branch_pill(tint))
    .into()
}

fn journal_button<'a>(label: String, message: Message) -> Element<'a, Message> {
    button(centered(text(label).size(12)))
        .padding([0, 10])
        .height(26)
        .on_press(message)
        .style(style::journal_button)
        .into()
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Splits `rtl/uart_rx.vhd` into (`Some("rtl")`, `uart_rx.vhd`).
fn split_path(path: &str) -> (Option<&str>, &str) {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((dir, _)) => (Some(dir), &path[dir.len() + 1..]),
        None => (None, path),
    }
}

fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Keeps at most `max_lines` lines of `s`.
fn clip(s: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= max_lines {
        s.to_string()
    } else {
        format!(
            "{}\n{}",
            lines[..max_lines].join("\n"),
            t!("journal.more_lines", count = lines.len() - max_lines)
        )
    }
}

/// Splits a console line into git arguments; a leading `git` is optional.
fn parse_console(input: &str) -> Result<Vec<String>, String> {
    let mut args = shell_words::split(input)
        .map_err(|e| t!("error.console_malformed", detail = e).into_owned())?;
    if args.first().map(String::as_str) == Some("git") {
        args.remove(0);
    }
    Ok(args)
}

async fn pick_folder(title: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .pick_folder()
        .await
        .map(|handle| handle.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_accepts_optional_git_prefix() {
        assert_eq!(
            parse_console("git log -n 2").unwrap(),
            vec!["log", "-n", "2"]
        );
        assert_eq!(
            parse_console("commit -m 'deux mots'").unwrap(),
            vec!["commit", "-m", "deux mots"]
        );
        assert!(parse_console("commit -m 'oops").is_err());
    }

    #[test]
    fn clip_limits_lines() {
        assert_eq!(clip("a\nb", 5), "a\nb");
        assert!(clip("a\nb\nc", 2).starts_with("a\nb\n"));
    }

    #[test]
    fn splits_paths_for_display() {
        assert_eq!(split_path("rtl/uart_rx.vhd"), (Some("rtl"), "uart_rx.vhd"));
        assert_eq!(split_path("top.vhd"), (None, "top.vhd"));
        assert_eq!(split_path("a/b/c.xdc"), (Some("a/b"), "c.xdc"));
        assert_eq!(split_path("build/"), (None, "build/"));
        assert_eq!(lowercase_first("Modifié"), "modifié");
    }
}
