//! The application state, its messages and its views.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::widget::{
    button, checkbox, column, container, row, rule, scrollable, space, text, text_input,
};
use iced::{Alignment, Element, Font, Length, Subscription, Task};

use crate::git::repo::{self, Commit, Operation, RepoPaths};
use crate::git::runner::{CommandRecord, GitCommand, run};
use crate::git::status::{EntryKind, FileEntry, Status};
use crate::i18n::{self, Lang, tr};

/// How many commits the history tab shows.
const LOG_LIMIT: usize = 200;
/// How many journal entries are kept.
const JOURNAL_LIMIT: usize = 500;
/// How often the open repository is re-read in the background.
const AUTO_REFRESH: Duration = Duration::from_secs(4);

#[derive(Default)]
pub struct App {
    lang: Option<Lang>,
    git_version: Option<String>,
    repo: Option<OpenRepo>,
    /// Every git command run so far, newest last.
    journal: Vec<CommandRecord>,
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
    tab: Tab,
    /// A background refresh is running; avoids piling them up.
    refreshing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Changes,
    History,
}

#[derive(Debug, Clone)]
pub enum Message {
    GitChecked(CommandRecord),
    PickRepoFolder,
    RepoFolderPicked(Option<PathBuf>),
    Opened(Result<RepoPaths, String>, CommandRecord),
    CloseRepo,
    Refresh,
    Tick,
    Refreshed {
        root: PathBuf,
        status: CommandRecord,
        log: CommandRecord,
    },
    SelectTab(Tab),
    CloneUrlChanged(String),
    PickCloneParent,
    CloneParentPicked(Option<PathBuf>),
    StartClone,
    Cloned(CommandRecord, PathBuf),
    ToggleAdvanced(bool),
    ToggleBackground(bool),
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
        let mut app = Self {
            lang: Some(lang),
            ..Self::default()
        };
        let check = Task::perform(run(GitCommand::new(["--version"])), Message::GitChecked);
        let open = match initial_dir {
            Some(dir) => app.open(dir),
            None => Task::none(),
        };
        (app, Task::batch([check, open]))
    }

    pub fn title(&self) -> String {
        match &self.repo {
            Some(repo) => t!("app.title_repo", name = folder_name(&repo.paths.root)).into_owned(),
            None => "Simple Git".to_string(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self.repo.is_some() && self.busy.is_none() {
            iced::time::every(AUTO_REFRESH).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
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
                            tab: Tab::Changes,
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
            Message::CloseRepo => {
                self.repo = None;
                Task::none()
            }
            Message::Refresh => self.refresh(false),
            Message::Tick => self.refresh(true),
            Message::Refreshed { root, status, log } => {
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
                self.log(status);
                self.log(log);
                Task::none()
            }
            Message::SelectTab(tab) => {
                if let Some(repo) = &mut self.repo {
                    repo.tab = tab;
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
        let mut status = repo::status_command(&root);
        let mut log = repo::log_command(&root, LOG_LIMIT);
        if background {
            status = status.background();
            log = log.background();
        }
        Task::perform(
            async move {
                let (status, log) = tokio::join!(run(status), run(log));
                Message::Refreshed { root, status, log }
            },
            |message| message,
        )
    }

    pub fn view(&self) -> Element<'_, Message> {
        let main: Element<'_, Message> = match &self.repo {
            None => self.welcome_view(),
            Some(repo) => self.repo_view(repo),
        };

        let mut layout = column![self.top_bar()].spacing(0);
        if let Some(error) = &self.error {
            layout = layout.push(
                container(
                    row![
                        text(error).width(Length::Fill),
                        button(text(t!("common.ok")))
                            .on_press(Message::DismissError)
                            .style(button::text),
                    ]
                    .align_y(Alignment::Center)
                    .spacing(10),
                )
                .padding(10)
                .width(Length::Fill)
                .style(container::danger),
            );
        }
        if let Some(busy) = &self.busy {
            layout = layout.push(
                container(text(busy))
                    .padding(10)
                    .width(Length::Fill)
                    .style(container::secondary),
            );
        }
        layout
            .push(container(main).padding(16).height(Length::FillPortion(3)))
            .push(rule::horizontal(1))
            .push(self.journal_view())
            .into()
    }

    fn top_bar(&self) -> Element<'_, Message> {
        let mut bar = row![text("Simple Git").size(20)]
            .spacing(12)
            .align_y(Alignment::Center);
        if let Some(repo) = &self.repo {
            bar = bar
                .push(
                    text(repo.paths.root.display().to_string())
                        .size(14)
                        .width(Length::Fill),
                )
                .push(
                    button(text(t!("toolbar.refresh")))
                        .on_press_maybe(self.idle().then_some(Message::Refresh))
                        .style(button::secondary),
                )
                .push(
                    button(text(t!("toolbar.close")))
                        .on_press_maybe(self.idle().then_some(Message::CloseRepo))
                        .style(button::secondary),
                );
        } else {
            bar = bar.push(space::horizontal());
        }
        for lang in Lang::ALL {
            bar = bar.push(
                button(text(lang.label()).size(13))
                    .on_press(Message::SetLanguage(lang))
                    .style(if self.lang == Some(lang) {
                        button::primary
                    } else {
                        button::text
                    }),
            );
        }
        bar = bar.push(
            checkbox(self.advanced)
                .label(t!("toolbar.advanced"))
                .on_toggle(Message::ToggleAdvanced),
        );
        container(bar)
            .padding([10, 16])
            .width(Length::Fill)
            .style(container::rounded_box)
            .into()
    }

    fn idle(&self) -> bool {
        self.busy.is_none()
    }

    fn welcome_view(&self) -> Element<'_, Message> {
        let open = column![
            text(t!("welcome.open_title")).size(18),
            text(t!("welcome.open_hint")),
            button(text(t!("welcome.pick_folder")))
                .on_press_maybe(self.idle().then_some(Message::PickRepoFolder)),
        ]
        .spacing(8);

        let parent = match &self.clone_parent {
            Some(p) => p.display().to_string(),
            None => t!("welcome.no_folder").into_owned(),
        };
        let can_clone =
            self.idle() && self.clone_parent.is_some() && !self.clone_url.trim().is_empty();
        let destination = match (&self.clone_parent, self.clone_url.trim()) {
            (Some(p), url) if !url.is_empty() => t!(
                "welcome.clone_destination",
                path = p.join(repo::clone_folder_name(url)).display()
            )
            .into_owned(),
            _ => String::new(),
        };
        let clone = column![
            text(t!("welcome.clone_title")).size(18),
            text(t!("welcome.clone_hint")),
            text_input(&t!("welcome.clone_placeholder"), &self.clone_url)
                .on_input(Message::CloneUrlChanged)
                .on_submit_maybe(can_clone.then_some(Message::StartClone)),
            row![
                button(text(t!("welcome.pick_parent")))
                    .on_press_maybe(self.idle().then_some(Message::PickCloneParent))
                    .style(button::secondary),
                text(parent),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            text(destination).size(14),
            button(text(t!("welcome.clone_button")))
                .on_press_maybe(can_clone.then_some(Message::StartClone)),
        ]
        .spacing(8);

        let version = text(
            self.git_version
                .clone()
                .unwrap_or_else(|| t!("welcome.git_searching").into_owned()),
        )
        .size(12);

        scrollable(
            column![open, rule::horizontal(1), clone, version]
                .spacing(24)
                .max_width(720),
        )
        .into()
    }

    fn repo_view<'a>(&'a self, repo: &'a OpenRepo) -> Element<'a, Message> {
        let mut header = column![].spacing(6);
        if let Some(status) = &repo.status {
            header = header.push(branch_summary(status));
        }
        if let Some(op) = repo.operation {
            header = header.push(
                container(text(tr(op.key())))
                    .padding(8)
                    .width(Length::Fill)
                    .style(container::warning),
            );
        }

        let tabs = row![
            tab_button(t!("repo.tab_changes"), Tab::Changes, repo.tab),
            tab_button(t!("repo.tab_history"), Tab::History, repo.tab),
        ]
        .spacing(4);

        let body = match repo.tab {
            Tab::Changes => match &repo.status {
                Some(status) => self.changes_view(status),
                None => text(t!("repo.reading_status")).into(),
            },
            Tab::History => self.history_view(&repo.commits),
        };

        column![header, tabs, body].spacing(12).into()
    }

    fn changes_view<'a>(&'a self, status: &'a Status) -> Element<'a, Message> {
        let conflicts: Vec<_> = status.conflicts().collect();
        let staged: Vec<_> = status.entries.iter().filter(|e| e.has_staged()).collect();
        let unstaged: Vec<_> = status
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Tracked && e.has_unstaged())
            .collect();
        let untracked: Vec<_> = status
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Untracked)
            .collect();

        if conflicts.is_empty() && staged.is_empty() && unstaged.is_empty() && untracked.is_empty()
        {
            return text(t!("changes.none")).into();
        }

        let mut list = column![].spacing(14);
        for (key, entries) in [
            ("changes.conflicts", conflicts),
            ("changes.staged", staged),
            ("changes.unstaged", unstaged),
            ("changes.untracked", untracked),
        ] {
            if entries.is_empty() {
                continue;
            }
            let mut group = column![
                row![
                    text(format!("{} ({})", tr(key), entries.len())).size(16),
                    text(tr(&format!("{key}_hint"))).size(12),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
            ]
            .spacing(4);
            for entry in entries {
                group = group.push(self.file_row(entry));
            }
            list = list.push(group);
        }
        scrollable(list).height(Length::Fill).into()
    }

    fn file_row<'a>(&'a self, entry: &'a FileEntry) -> Element<'a, Message> {
        let path = match &entry.orig_path {
            Some(orig) => format!("{orig} → {}", entry.path),
            None => entry.path.clone(),
        };
        let mut line = row![
            text(path).font(Font::MONOSPACE).width(Length::Fill),
            text(tr(entry.state_key())).size(14),
        ]
        .spacing(12);
        if self.advanced {
            line = line.push(text(entry.xy.clone()).font(Font::MONOSPACE).size(12));
        }
        container(line).padding([2, 8]).into()
    }

    fn history_view<'a>(&'a self, commits: &'a [Commit]) -> Element<'a, Message> {
        if commits.is_empty() {
            return text(t!("history.none")).into();
        }
        let mut list = column![].spacing(2);
        for commit in commits {
            let mut line = row![text(commit.short.clone()).font(Font::MONOSPACE).size(13)]
                .spacing(10)
                .align_y(Alignment::Center);
            for r in &commit.refs {
                line = line.push(
                    container(text(r.clone()).size(12))
                        .padding([1, 6])
                        .style(container::primary),
                );
            }
            line = line
                .push(text(commit.subject.clone()).width(Length::Fill))
                .push(text(commit.author.clone()).size(13))
                .push(text(commit.date.clone()).size(13));
            list = list.push(line);
        }
        scrollable(list).height(Length::Fill).into()
    }

    fn journal_view(&self) -> Element<'_, Message> {
        let mut header = row![
            text(t!("journal.title")).size(16),
            text(t!("journal.hint")).size(12),
            space::horizontal(),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if self.advanced {
            header = header.push(
                checkbox(self.show_background)
                    .label(t!("journal.show_background"))
                    .on_toggle(Message::ToggleBackground),
            );
        }
        header = header.push(
            button(text(t!("journal.clear")))
                .on_press(Message::ClearJournal)
                .style(button::text),
        );

        let mut entries = column![].spacing(6);
        for record in self
            .journal
            .iter()
            .filter(|r| self.show_background || !r.command.background)
        {
            entries = entries.push(self.journal_entry(record));
        }

        let mut panel = column![
            header,
            scrollable(entries)
                .anchor_bottom()
                .width(Length::Fill)
                .height(Length::Fill)
        ]
        .spacing(6);

        if self.advanced {
            panel = panel.push(
                row![
                    text("git").font(Font::MONOSPACE),
                    text_input(&t!("journal.console_placeholder"), &self.console_input)
                        .font(Font::MONOSPACE)
                        .on_input(Message::ConsoleChanged)
                        .on_submit_maybe(self.idle().then_some(Message::ConsoleSubmit)),
                    button(text(t!("journal.run")))
                        .on_press_maybe(self.idle().then_some(Message::ConsoleSubmit)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }

        container(panel)
            .padding([8, 16])
            .height(Length::FillPortion(2))
            .width(Length::Fill)
            .into()
    }

    fn journal_entry<'a>(&'a self, record: &'a CommandRecord) -> Element<'a, Message> {
        let mark = if record.success() { "✓" } else { "✗" };
        let mut line = row![
            text(mark),
            text(format!("$ {}", record.command.display()))
                .font(Font::MONOSPACE)
                .size(13)
                .width(Length::Fill),
            text(format!("{} ms", record.duration.as_millis())).size(12),
        ]
        .spacing(8);
        if let Some(dir) = record.command.cwd.as_deref().filter(|_| self.advanced) {
            line = line.push(text(folder_name(dir)).size(12));
        }
        let mut entry = column![line].spacing(2);
        let show_output = self.advanced || !record.success();
        if show_output {
            let output = clip(record.stdout.trim_end(), 20);
            let errors = clip(record.stderr.trim_end(), 20);
            if !output.is_empty() && self.advanced {
                entry = entry.push(text(output).font(Font::MONOSPACE).size(12));
            }
            if !errors.is_empty() {
                entry = entry.push(text(errors).font(Font::MONOSPACE).size(12));
            }
            if !record.success() && record.stderr.trim().is_empty() {
                entry = entry.push(text(record.error_summary()).size(12));
            }
        }
        entry.into()
    }
}

fn branch_summary(status: &Status) -> Element<'_, Message> {
    let branch = &status.branch;
    let name = match (&branch.head, &branch.oid) {
        (Some(head), _) => t!("branch.current", name = head),
        (None, Some(oid)) => t!("branch.detached", oid = &oid[..oid.len().min(8)]),
        (None, None) => t!("branch.none"),
    };
    let sync = match &branch.upstream {
        None if branch.oid.is_none() => t!("sync.no_commits"),
        None => t!("sync.unpublished"),
        Some(up) => match (branch.ahead, branch.behind) {
            (0, 0) => t!("sync.up_to_date", upstream = up),
            (a, 0) => t!("sync.ahead", ahead = a, upstream = up),
            (0, b) => t!("sync.behind", behind = b, upstream = up),
            (a, b) => t!("sync.both", ahead = a, behind = b, upstream = up),
        },
    };
    row![text(name).size(18), text(sync).size(14)]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
}

fn tab_button(label: Cow<'_, str>, tab: Tab, current: Tab) -> Element<'_, Message> {
    button(text(label))
        .on_press(Message::SelectTab(tab))
        .style(if tab == current {
            button::primary
        } else {
            button::secondary
        })
        .into()
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
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
}
