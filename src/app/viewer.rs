//! Large views over the whole window: a file's diff (optionally the whole
//! file with its changes), and the project as it was at a commit, with its
//! folders and the content of any file.

use std::collections::BTreeSet;

use iced::widget::{column, row};

use super::changes::{code, hunk_range, number, side_by_side, unified};
use super::*;
use crate::git::branches::Comparison;
use crate::git::diff::{self, FileDiff};
use crate::ui::syntax;

/// Lines drawn at most in a large view.
const MAX_LINES: usize = 10_000;
/// Files larger than this are not shown in the project view.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Rows of the file list drawn at most.
const MAX_LIST_ROWS: usize = 2_000;
/// Enough context lines for git to print the whole file around the changes.
const WHOLE_FILE: &str = "--unified=1000000";

pub enum Viewer {
    Diff(DiffViewer),
    Tree(TreeViewer),
}

impl Viewer {
    fn root(&self) -> &Path {
        match self {
            Viewer::Diff(v) => &v.root,
            Viewer::Tree(v) => &v.root,
        }
    }
}

/// Where the diff shown large comes from.
#[derive(Debug, Clone)]
pub enum Source {
    /// The changes on disk, staged or not.
    WorkTree { staged: bool, untracked: bool },
    /// A commit or a comparison of the history.
    History(Comparison),
}

pub struct DiffViewer {
    root: PathBuf,
    path: String,
    source: Source,
    /// The whole file with its changes, not only the changed parts.
    whole: bool,
    /// `None` while loading.
    diff: Option<FileDiff>,
    command: Option<String>,
}

pub struct TreeViewer {
    root: PathBuf,
    hash: String,
    /// Every file of the commit, in git's order (folders together).
    files: Vec<String>,
    loaded: bool,
    filter: String,
    /// Folders opened in the list; the others are folded.
    expanded: BTreeSet<String>,
    selected: Option<String>,
    content: Option<Content>,
}

#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    Binary,
    TooLarge(u64),
    Unreadable(String),
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The file selected in "Changes", large.
    OpenChanges,
    /// The file selected in a commit's details, large.
    OpenCommitFile,
    Whole(bool),
    DiffLoaded(PathBuf, String, bool, Box<FileDiff>, String, CommandRecord),
    /// The project at this commit.
    Browse(String),
    TreeLoaded(PathBuf, String, Vec<String>, CommandRecord),
    Filter(String),
    ToggleFolder(String),
    SelectFile(String),
    FileLoaded(PathBuf, String, Box<Content>, Vec<CommandRecord>),
    Close,
}

/// `git ls-tree`: every file of a commit, NUL-separated.
fn tree_command(root: &Path, hash: &str) -> GitCommand {
    GitCommand::new(["ls-tree", "-r", "-z", "--name-only", hash]).in_dir(root)
}

/// The same diff with the whole file around the changes (`git diff` and
/// `git diff-tree -p` both take the option right after their name).
fn whole_file(mut command: GitCommand) -> GitCommand {
    command.args.insert(1, WHOLE_FILE.to_string());
    command
}

fn object(hash: &str, path: &str) -> String {
    format!("{hash}:{path}")
}

async fn load_file(root: PathBuf, hash: String, path: String) -> (Content, Vec<CommandRecord>) {
    let spec = object(&hash, &path);
    let size = run(GitCommand::new(["cat-file", "-s", spec.as_str()]).in_dir(&root)).await;
    if !size.success() {
        let error = size.error_summary();
        return (Content::Unreadable(error), vec![size]);
    }
    let bytes: u64 = size.stdout.trim().parse().unwrap_or(0);
    if bytes > MAX_FILE_BYTES {
        return (Content::TooLarge(bytes), vec![size]);
    }
    let mut show = run(GitCommand::new(["show", spec.as_str()]).in_dir(&root)).await;
    let content = if !show.success() {
        Content::Unreadable(show.error_summary())
    } else if show.stdout.contains('\0') {
        Content::Binary
    } else {
        // The text lives in the view only, not twice in the journal.
        Content::Text(std::mem::take(&mut show.stdout))
    };
    (content, vec![size, show])
}

impl App {
    pub(super) fn viewer_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::OpenChanges => {
                let Some(repo) = &self.repo else {
                    return Task::none();
                };
                let Some(view) = &repo.diff else {
                    return Task::none();
                };
                let untracked = matches!(view.kind, super::changes::DiffKind::Untracked);
                self.viewer = Some(Viewer::Diff(DiffViewer {
                    root: repo.paths.root.clone(),
                    path: view.selection.path.clone(),
                    source: Source::WorkTree {
                        staged: view.selection.staged,
                        untracked,
                    },
                    whole: untracked,
                    diff: Some(view.diff.clone()),
                    command: view.command.clone(),
                }));
                Task::none()
            }
            Msg::OpenCommitFile => {
                let Some(repo) = &self.repo else {
                    return Task::none();
                };
                let Some(details) = &repo.history.details else {
                    return Task::none();
                };
                let Some(path) = details.file.clone() else {
                    return Task::none();
                };
                self.viewer = Some(Viewer::Diff(DiffViewer {
                    root: repo.paths.root.clone(),
                    path,
                    source: Source::History(details.comparison.clone()),
                    whole: false,
                    diff: details.diff.clone(),
                    command: details.command.clone(),
                }));
                Task::none()
            }
            Msg::Whole(whole) => {
                let options = self.diff_options;
                let Some(Viewer::Diff(v)) = &mut self.viewer else {
                    return Task::none();
                };
                v.whole = whole;
                v.diff = None;
                let mut command = match &v.source {
                    Source::WorkTree { staged, .. } => {
                        diff::diff_command(&v.root, &v.path, *staged, options)
                    }
                    Source::History(comparison) => crate::git::branches::file_diff_command(
                        &v.root,
                        comparison,
                        &v.path,
                        options.ignore_whitespace,
                    ),
                };
                if whole {
                    command = whole_file(command);
                }
                let shown = command.display();
                let (root, path) = (v.root.clone(), v.path.clone());
                let detect_moves = options.detect_moves;
                Task::perform(
                    async move {
                        let record = run(command).await;
                        let diff = diff::parse(&record.stdout, detect_moves);
                        (diff, record)
                    },
                    move |(diff, record)| {
                        Message::Viewer(Msg::DiffLoaded(
                            root.clone(),
                            path.clone(),
                            whole,
                            Box::new(diff),
                            shown.clone(),
                            record,
                        ))
                    },
                )
            }
            Msg::DiffLoaded(root, path, whole, diff, command, record) => {
                self.log(record);
                if let Some(Viewer::Diff(v)) = &mut self.viewer
                    && v.root == root
                    && v.path == path
                    && v.whole == whole
                {
                    v.diff = Some(*diff);
                    v.command = Some(command);
                }
                Task::none()
            }
            Msg::Browse(hash) => {
                self.context_menu = None;
                let Some(repo) = &self.repo else {
                    return Task::none();
                };
                let root = repo.paths.root.clone();
                self.viewer = Some(Viewer::Tree(TreeViewer {
                    root: root.clone(),
                    hash: hash.clone(),
                    files: Vec::new(),
                    loaded: false,
                    filter: String::new(),
                    expanded: BTreeSet::new(),
                    selected: None,
                    content: None,
                }));
                let command = tree_command(&root, &hash);
                Task::perform(run(command), move |mut record| {
                    let files = record
                        .stdout
                        .split('\0')
                        .filter(|p| !p.is_empty())
                        .map(str::to_string)
                        .collect();
                    record.stdout.clear();
                    Message::Viewer(Msg::TreeLoaded(root.clone(), hash.clone(), files, record))
                })
            }
            Msg::TreeLoaded(root, hash, files, record) => {
                let failed = !record.success();
                let error = record.error_summary();
                self.log(record);
                if let Some(Viewer::Tree(v)) = &mut self.viewer
                    && v.root == root
                    && v.hash == hash
                {
                    v.files = files;
                    v.loaded = true;
                    if failed {
                        v.content = Some(Content::Unreadable(error));
                    }
                }
                Task::none()
            }
            Msg::Filter(filter) => {
                if let Some(Viewer::Tree(v)) = &mut self.viewer {
                    v.filter = filter;
                }
                Task::none()
            }
            Msg::ToggleFolder(folder) => {
                if let Some(Viewer::Tree(v)) = &mut self.viewer
                    && !v.expanded.remove(&folder)
                {
                    v.expanded.insert(folder);
                }
                Task::none()
            }
            Msg::SelectFile(path) => {
                let Some(Viewer::Tree(v)) = &mut self.viewer else {
                    return Task::none();
                };
                v.selected = Some(path.clone());
                v.content = None;
                let (root, hash) = (v.root.clone(), v.hash.clone());
                Task::perform(
                    load_file(root.clone(), hash, path.clone()),
                    move |(content, records)| {
                        Message::Viewer(Msg::FileLoaded(
                            root.clone(),
                            path.clone(),
                            Box::new(content),
                            records,
                        ))
                    },
                )
            }
            Msg::FileLoaded(root, path, content, records) => {
                for record in records {
                    self.log(record);
                }
                if let Some(Viewer::Tree(v)) = &mut self.viewer
                    && v.root == root
                    && v.selected.as_deref() == Some(path.as_str())
                {
                    v.content = Some(*content);
                }
                Task::none()
            }
            Msg::Close => {
                self.viewer = None;
                Task::none()
            }
        }
    }

    /// The large view, if one is open on the current repository.
    pub(super) fn open_viewer(&self) -> Option<&Viewer> {
        let repo = self.repo.as_ref()?;
        self.viewer.as_ref().filter(|v| v.root() == repo.paths.root)
    }

    // --------------------------------------------------------------- views

    pub(super) fn viewer_view<'a>(
        &'a self,
        viewer: &'a Viewer,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let body = match viewer {
            Viewer::Diff(v) => self.diff_viewer(v, p),
            Viewer::Tree(v) => self.tree_viewer(v, p),
        };
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::card)
            .into()
    }

    fn diff_viewer<'a>(&'a self, v: &'a DiffViewer, p: &'static Palette) -> Element<'a, Message> {
        let what = match &v.source {
            Source::WorkTree { staged: true, .. } => t!("diff.staged_against").into_owned(),
            Source::WorkTree { .. } => t!("diff.unstaged_against").into_owned(),
            Source::History(Comparison::Commit { hash, .. }) => {
                t!("viewer.in_commit", commit = short(hash)).into_owned()
            }
            Source::History(Comparison::Between { from, to }) => {
                t!("viewer.between", from = short(from), to = short(to)).into_owned()
            }
            Source::History(Comparison::WithWorkingTree { hash }) => {
                t!("viewer.with_files", commit = short(hash)).into_owned()
            }
        };
        let untracked = matches!(
            v.source,
            Source::WorkTree {
                untracked: true,
                ..
            }
        );

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
        let mut header = row![
            text(v.path.clone()).size(14).font(mono(Weight::Medium)),
            text(what).size(12).color(p.muted).width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if !untracked {
            header = header.push(
                checkbox(v.whole)
                    .label(t!("viewer.whole_file"))
                    .size(14)
                    .text_size(12)
                    .on_toggle_maybe(
                        v.diff
                            .is_some()
                            .then_some(|w| Message::Viewer(Msg::Whole(w))),
                    )
                    .style(style::check),
            );
        }
        let header = header
            .push(container(views).padding(1).style(style::segmented_frame))
            .push(close_button());

        let lang = syntax::language_for(&v.path);
        let body: Element<'a, Message> = match &v.diff {
            None => note(t!("diff.loading").into_owned(), p),
            Some(d) if d.binary => note(t!("diff.binary").into_owned(), p),
            Some(d) if d.is_empty() => note(t!("diff.empty").into_owned(), p),
            Some(d) => {
                let mut col = column![];
                let mut budget = MAX_LINES;
                for hunk in &d.hunks {
                    if !v.whole {
                        let (from, to) = hunk_range(hunk);
                        let title = if hunk.section.is_empty() {
                            t!("diff.lines", from = from, to = to).into_owned()
                        } else {
                            format!(
                                "{} · {}",
                                hunk.section,
                                t!("diff.lines", from = from, to = to)
                            )
                        };
                        col = col.push(
                            container(text(title).size(12))
                                .padding([6, 14])
                                .width(Length::Fill)
                                .style(style::hunk_header),
                        );
                    }
                    let shown = hunk.lines.len().min(budget);
                    col = col.push(if self.side_by_side {
                        side_by_side(&hunk.lines[..shown], lang, p)
                    } else {
                        unified(&hunk.lines[..shown], lang, p)
                    });
                    budget -= shown;
                    if budget == 0 {
                        col = col.push(truncated(p));
                        break;
                    }
                }
                scrollable(col)
                    .height(Length::Fill)
                    .style(style::scroll)
                    .into()
            }
        };
        let footer = v
            .command
            .as_ref()
            .map(|c| format!("$ {c}"))
            .unwrap_or_default();
        column![
            container(header).padding([10, 14]),
            hline(style::separator),
            container(body).height(Length::Fill),
            hline(style::separator),
            container(text(footer).size(11).font(style::MONO).color(p.text_2)).padding([8, 14]),
        ]
        .into()
    }

    fn tree_viewer<'a>(&'a self, v: &'a TreeViewer, p: &'static Palette) -> Element<'a, Message> {
        let subject = self
            .repo
            .as_ref()
            .and_then(|r| r.commits.iter().find(|c| c.hash == v.hash))
            .map(|c| c.subject.clone())
            .unwrap_or_default();
        let header = row![
            text(t!("viewer.project_at", commit = short(&v.hash)))
                .size(15)
                .font(sans(Weight::Semibold)),
            text(subject)
                .size(13)
                .color(p.text_2)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill),
            close_button(),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let list = column![
            container(
                text_input(&t!("viewer.filter"), &v.filter)
                    .size(13)
                    .padding([6, 10])
                    .on_input(|f| Message::Viewer(Msg::Filter(f)))
                    .style(style::input),
            )
            .padding(10),
            hline(style::separator),
            scrollable(self.file_rows(v, p))
                .height(Length::Fill)
                .style(style::scroll),
        ]
        .width(360);

        let content: Element<'a, Message> = match (&v.selected, &v.content) {
            (None, Some(Content::Unreadable(e))) => note(e.clone(), p),
            (None, _) if !v.loaded => note(t!("diff.loading").into_owned(), p),
            (None, _) => note(t!("viewer.pick_file").into_owned(), p),
            (Some(_), None) => note(t!("diff.loading").into_owned(), p),
            (Some(_), Some(Content::Binary)) => note(t!("diff.binary").into_owned(), p),
            (Some(_), Some(Content::TooLarge(bytes))) => note(
                t!(
                    "viewer.too_large",
                    size = format!("{:.1}", *bytes as f64 / 1e6)
                )
                .into_owned(),
                p,
            ),
            (Some(_), Some(Content::Unreadable(e))) => note(e.clone(), p),
            (Some(path), Some(Content::Text(body))) => file_lines(path, body, p),
        };
        let command = v.selected.as_ref().map_or_else(
            || tree_command(&v.root, &v.hash).display(),
            |path| GitCommand::new(["show".to_string(), object(&v.hash, path)]).display(),
        );
        let right = column![
            container(
                text(v.selected.clone().unwrap_or_default())
                    .size(13)
                    .font(mono(Weight::Medium))
            )
            .padding([12, 14]),
            hline(style::separator),
            container(content).height(Length::Fill),
            hline(style::separator),
            container(
                text(format!("$ {command}"))
                    .size(11)
                    .font(style::MONO)
                    .color(p.text_2)
            )
            .padding([8, 14]),
        ]
        .width(Length::Fill);

        column![
            container(header).padding([10, 14]),
            hline(style::separator),
            row![list, vline(), right].height(Length::Fill),
        ]
        .into()
    }

    /// The folders and files of the commit; folders open on click. A filter
    /// shows every matching file directly.
    fn file_rows<'a>(&'a self, v: &'a TreeViewer, p: &'static Palette) -> Element<'a, Message> {
        let mut col = column![].padding([6, 6]);
        let mut count = 0usize;
        let file_row = |path: &str, label: String, depth: usize| -> Element<'a, Message> {
            let selected = v.selected.as_deref() == Some(path);
            button(
                text(label)
                    .size(13)
                    .font(style::MONO)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .padding(Padding::from([4, 10]).left(14.0 + 16.0 * depth as f32))
            .width(Length::Fill)
            .on_press(Message::Viewer(Msg::SelectFile(path.to_string())))
            .style(move |theme, status| {
                if selected {
                    style::choice(true)(theme, status)
                } else {
                    style::menu_item(theme, status)
                }
            })
            .into()
        };

        let filter = v.filter.trim().to_lowercase();
        if !filter.is_empty() {
            for path in v
                .files
                .iter()
                .filter(|f| f.to_lowercase().contains(&filter))
            {
                if count == MAX_LIST_ROWS {
                    break;
                }
                col = col.push(file_row(path, path.clone(), 0));
                count += 1;
            }
            if count == 0 && v.loaded {
                col = col.push(
                    container(text(t!("viewer.no_match")).size(13).color(p.muted)).padding(14),
                );
            }
            return col.into();
        }

        // Folders already listed, from the root down, for the current file.
        let mut open: Vec<&str> = Vec::new();
        for path in &v.files {
            if count >= MAX_LIST_ROWS {
                col = col.push(
                    container(text(t!("viewer.use_filter")).size(12).color(p.muted)).padding(14),
                );
                break;
            }
            let parts: Vec<&str> = path.split('/').collect();
            let (name, folders) = parts.split_last().expect("split gives one part");
            let common = open
                .iter()
                .zip(folders.iter())
                .take_while(|(a, b)| a == b)
                .count();
            open.truncate(common);
            let mut visible = folders[..common]
                .iter()
                .enumerate()
                .all(|(depth, _)| v.expanded.contains(&folders[..=depth].join("/")));
            for depth in common..folders.len() {
                open.push(folders[depth]);
                if !visible {
                    continue;
                }
                let folder = folders[..=depth].join("/");
                let expanded = v.expanded.contains(&folder);
                col = col.push(
                    button(
                        row![
                            text(if expanded { "▾" } else { "▸" })
                                .size(12)
                                .color(p.muted)
                                .width(12),
                            text(format!("{}/", folders[depth]))
                                .size(13)
                                .font(mono(Weight::Medium))
                                .wrapping(iced::widget::text::Wrapping::None),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding(Padding::from([4, 10]).left(14.0 + 16.0 * depth as f32))
                    .width(Length::Fill)
                    .on_press(Message::Viewer(Msg::ToggleFolder(folder.clone())))
                    .style(style::menu_item),
                );
                count += 1;
                visible = expanded;
            }
            if visible {
                col = col.push(file_row(path, (*name).to_string(), folders.len()));
                count += 1;
            }
        }
        col.into()
    }
}

/// A file's lines with their numbers, colored by language.
fn file_lines<'a>(path: &str, body: &'a str, p: &'static Palette) -> Element<'a, Message> {
    let lang = syntax::language_for(path);
    let mut state = syntax::State::Normal;
    let mut col = column![];
    for (i, line) in body.lines().enumerate() {
        if i == MAX_LINES {
            col = col.push(truncated(p));
            break;
        }
        let pieces = match lang {
            Some(lang) => syntax::highlight(lang, line, &mut state),
            None => Vec::new(),
        };
        col = col.push(
            row![
                number(Some(i as u32 + 1), p.line_number),
                container(code(line, &pieces, p.text, p)).padding(Padding::default().left(12)),
            ]
            .height(Length::Shrink),
        );
    }
    scrollable(container(col).padding([6, 0]))
        .height(Length::Fill)
        .style(style::scroll)
        .into()
}

fn close_button<'a>() -> Element<'a, Message> {
    button(centered(text(t!("common.close")).size(12)))
        .padding([0, 12])
        .height(28)
        .on_press(Message::Viewer(Msg::Close))
        .style(style::secondary)
        .into()
}

fn note<'a>(note: String, p: &'static Palette) -> Element<'a, Message> {
    container(text(note).color(p.muted))
        .center(Length::Fill)
        .padding(20)
        .into()
}

fn truncated<'a>(p: &'static Palette) -> Element<'a, Message> {
    container(
        text(t!("viewer.truncated", count = MAX_LINES))
            .size(12)
            .color(p.muted),
    )
    .padding([8, 14])
    .into()
}

fn short(hash: &str) -> String {
    hash.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn git(dir: &Path, args: &[&str]) -> String {
        let record = run(GitCommand::new(args.iter().copied()).in_dir(dir)).await;
        assert!(record.success(), "git {args:?}: {}", record.stderr);
        record.stdout
    }

    #[tokio::test]
    async fn whole_file_diff_tree_and_file_content() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git(root, &["init", "-q", "-b", "main"]).await;
        git(root, &["config", "user.name", "Test"]).await;
        git(root, &["config", "user.email", "t@example.com"]).await;
        git(root, &["config", "core.autocrlf", "false"]).await;
        let lines: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
        std::fs::create_dir(root.join("rtl")).unwrap();
        std::fs::write(root.join("rtl/top.vhd"), lines.join("\n") + "\n").unwrap();
        std::fs::write(root.join("logo.bin"), [0u8, 1, 2, 0]).unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "first"]).await;
        let hash = git(root, &["rev-parse", "HEAD"]).await.trim().to_string();

        let mut changed = lines.clone();
        changed[14] = "line fifteen".to_string();
        std::fs::write(root.join("rtl/top.vhd"), changed.join("\n") + "\n").unwrap();
        let options = DiffOptions::default();
        let usual = run(diff::diff_command(root, "rtl/top.vhd", false, options)).await;
        let usual = diff::parse(&usual.stdout, true);
        assert!(usual.hunks[0].lines.len() < 10);
        let whole = run(whole_file(diff::diff_command(
            root,
            "rtl/top.vhd",
            false,
            options,
        )))
        .await;
        let whole = diff::parse(&whole.stdout, true);
        assert_eq!(whole.hunks.len(), 1);
        // 30 lines, the changed one twice (removed and added).
        assert_eq!(whole.hunks[0].lines.len(), 31);

        let tree = run(tree_command(root, &hash)).await;
        let files: Vec<&str> = tree.stdout.split('\0').filter(|p| !p.is_empty()).collect();
        assert_eq!(files, ["logo.bin", "rtl/top.vhd"]);

        let (content, _) = load_file(root.to_path_buf(), hash.clone(), "rtl/top.vhd".into()).await;
        match content {
            Content::Text(text) => assert_eq!(text, lines.join("\n") + "\n"),
            other => panic!("{other:?}"),
        }
        let (content, _) = load_file(root.to_path_buf(), hash, "logo.bin".into()).await;
        assert!(matches!(content, Content::Binary), "{content:?}");
    }
}
