//! The merge and rebase assistant: a first dialog that explains the
//! operation and shows its command, then, while it is in progress, a screen
//! to settle each conflict with both sides named after their branches, to
//! check the result for blocks kept twice, and to save or stop.

use iced::widget::{column, row, text_editor};

use super::undo::Undoable;
use super::*;
use crate::git::conflict::{self, Choice, Duplicate, Part};
pub use crate::git::integrate::Kind;
use crate::git::integrate::{self, Sides};
use crate::git::undo::{self as git_undo, Entry, Moves, State};
use crate::ui::syntax;

/// The dialog shown before starting a merge or a rebase.
#[derive(Debug, Clone)]
pub struct Prompt {
    pub kind: Kind,
    /// The other branch (or a commit, for a rebase onto it).
    pub other: String,
    /// For a rebase that moves only the commits after this one (`--onto`).
    pub after: Option<String>,
    pub ignore_whitespace: bool,
}

/// A merge or rebase started from the app, remembered so it can be undone
/// as a whole once finished.
#[derive(Debug, Clone)]
pub struct Session {
    pub label: String,
    pub before: State,
    pub kind: Kind,
    pub other: String,
    /// Files whose conflicts were settled here.
    pub settled: Vec<String>,
}

/// A file of the operation in progress.
#[derive(Debug)]
pub struct ResolverFile {
    pub path: String,
    /// Still in conflict (otherwise: merged by git, to review).
    pub conflicted: bool,
    /// The file as git left it, with conflict markers.
    pub content: String,
    /// `None` when the file can't be split into conflicts (binary, deleted
    /// on one side, markers edited by hand).
    pub parts: Option<Vec<Part>>,
    pub choices: Vec<Option<Choice>>,
    /// Base, current and incoming versions.
    pub versions: [String; 3],
    pub show_base: bool,
    /// A conflict being written by hand: its index and the text.
    pub editing: Option<(usize, text_editor::Content)>,
    /// Blocks found twice in the result.
    pub duplicates: Vec<Duplicate>,
}

impl Clone for ResolverFile {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            conflicted: self.conflicted,
            content: self.content.clone(),
            parts: self.parts.clone(),
            choices: self.choices.clone(),
            versions: self.versions.clone(),
            show_base: self.show_base,
            editing: self
                .editing
                .as_ref()
                .map(|(i, c)| (*i, text_editor::Content::with_text(&c.text()))),
            duplicates: self.duplicates.clone(),
        }
    }
}

impl ResolverFile {
    fn settled(&self) -> usize {
        self.choices.iter().filter(|c| c.is_some()).count()
    }

    fn total(&self) -> usize {
        self.choices.len()
    }

    /// The file once every conflict is settled.
    fn result(&self) -> Option<String> {
        conflict::resolve(self.parts.as_ref()?, &self.choices)
    }

    fn refresh_duplicates(&mut self) {
        let merged = if self.conflicted {
            self.result()
        } else {
            Some(self.content.clone())
        };
        self.duplicates = match merged {
            Some(m) => conflict::duplicated_blocks(
                &self.versions[0],
                &self.versions[1],
                &self.versions[2],
                &m,
            ),
            None => Vec::new(),
        };
    }
}

/// What the conflict screen shows for the operation in progress.
#[derive(Debug, Clone)]
pub struct Resolver {
    /// Changes when the operation moves to another step (new conflicts).
    pub key: String,
    pub operation: Operation,
    pub sides: Sides,
    pub files: Vec<ResolverFile>,
    pub selected: Option<String>,
}

/// Blocks found twice after a rebase finished.
#[derive(Debug, Clone)]
pub struct AfterCheck {
    pub sides: Sides,
    pub files: Vec<(String, Vec<Duplicate>)>,
}

/// Identifies the current step of an operation: the conflicted files and
/// the commit being replayed.
pub(super) fn resolver_key(repo: &OpenRepo) -> Option<String> {
    let op = repo.operation?;
    let status = repo.status.as_ref()?;
    let mut paths: Vec<&str> = status.conflicts().map(|e| e.path.as_str()).collect();
    paths.sort_unstable();
    let step = [
        "REBASE_HEAD",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
    ]
    .iter()
    .filter_map(|f| std::fs::read_to_string(repo.paths.git_dir.join(f)).ok())
    .collect::<String>();
    Some(format!("{op:?}|{}|{}", paths.join("|"), step.trim()))
}

async fn load_resolver(
    root: PathBuf,
    git_dir: PathBuf,
    operation: Operation,
    branch: Option<String>,
    conflicted: Vec<String>,
    key: String,
) -> (Resolver, Vec<CommandRecord>) {
    let (sides, mut records) =
        integrate::sides(&root, &git_dir, operation, branch.as_deref()).await;
    let mut files = Vec::new();
    for path in &conflicted {
        let content = std::fs::read_to_string(root.join(path)).unwrap_or_default();
        let (versions, r) = integrate::stage_versions(&root, path).await;
        records.extend(r);
        let parts = conflict::parse(&content).filter(|p| conflict::conflict_count(p) > 0);
        let count = parts.as_deref().map_or(0, conflict::conflict_count);
        files.push(ResolverFile {
            path: path.clone(),
            conflicted: true,
            content,
            parts,
            choices: vec![None; count],
            versions,
            show_base: false,
            editing: None,
            duplicates: Vec::new(),
        });
    }
    // A merge: also check the files git merged by itself.
    if operation == Operation::Merge {
        let base = run(integrate::merge_base(&root, "HEAD", "MERGE_HEAD")).await;
        let base_hash = base.stdout.trim().to_string();
        records.push(base);
        if !base_hash.is_empty() {
            let (both, r) =
                integrate::touched_by_both(&root, &base_hash, "HEAD", "MERGE_HEAD").await;
            records.extend(r);
            for path in both.into_iter().filter(|p| !conflicted.contains(p)) {
                let (versions, r) = integrate::commit_versions(
                    &root,
                    [base_hash.as_str(), "HEAD", "MERGE_HEAD"],
                    &path,
                )
                .await;
                records.extend(r);
                let content = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
                let mut file = ResolverFile {
                    path,
                    conflicted: false,
                    content,
                    parts: None,
                    choices: Vec::new(),
                    versions,
                    show_base: false,
                    editing: None,
                    duplicates: Vec::new(),
                };
                file.refresh_duplicates();
                files.push(file);
            }
        }
    }
    let selected = files.first().map(|f| f.path.clone());
    (
        Resolver {
            key,
            operation,
            sides,
            files,
            selected,
        },
        records,
    )
}

/// After a rebase: blocks kept twice in the files both sides changed.
async fn check_after_rebase(
    root: PathBuf,
    onto: String,
    branch: String,
) -> (Option<AfterCheck>, Vec<CommandRecord>) {
    let mut records = Vec::new();
    let base = run(integrate::merge_base(&root, "ORIG_HEAD", &onto)).await;
    let base_hash = base.stdout.trim().to_string();
    records.push(base);
    if base_hash.is_empty() {
        return (None, records);
    }
    let (both, r) = integrate::touched_by_both(&root, &base_hash, &onto, "ORIG_HEAD").await;
    records.extend(r);
    let mut files = Vec::new();
    for path in both {
        let (v, r) =
            integrate::commit_versions(&root, [base_hash.as_str(), &onto, "ORIG_HEAD"], &path)
                .await;
        records.extend(r);
        let merged = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
        let found = conflict::duplicated_blocks(&v[0], &v[1], &v[2], &merged);
        if !found.is_empty() {
            files.push((path, found));
        }
    }
    let check = (!files.is_empty()).then_some(AfterCheck {
        sides: Sides {
            current: onto,
            incoming: branch,
            replaying: None,
        },
        files,
    });
    (check, records)
}

impl App {
    // ------------------------------------------------------------ updates

    pub(super) fn open_integration(&mut self, kind: Kind, other: String) -> Task<Message> {
        if self.repo.as_ref().is_some_and(|r| r.operation.is_some()) {
            return Task::none();
        }
        self.integration_prompt = Some(Prompt {
            kind,
            other,
            after: None,
            ignore_whitespace: true,
        });
        Task::none()
    }

    pub(super) fn start_integration(&mut self) -> Task<Message> {
        let Some(prompt) = self.integration_prompt.take() else {
            return Task::none();
        };
        let (current, _) = self.current_branch_and_changes();
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let current = current.unwrap_or_else(|| "HEAD".to_string());
        let label = match prompt.kind {
            Kind::Merge => t!("undo.merge", branch = prompt.other, into = current),
            Kind::Rebase => t!("undo.rebase", branch = current, onto = prompt.other),
        }
        .into_owned();
        repo.after_check = None;
        let command = integrate::start(
            &root,
            prompt.kind,
            &prompt.other,
            prompt.after.as_deref(),
            prompt.ignore_whitespace,
        );
        self.busy = Some(t!("busy.integrating").into_owned());
        self.notice = None;
        let kind = prompt.kind;
        let other = prompt.other.clone();
        Task::perform(
            async move {
                let (before, mut records) = git_undo::snapshot(&root).await;
                records.push(run(command).await);
                Message::IntegrationStarted(
                    Box::new(Session {
                        label,
                        before,
                        kind,
                        other,
                        settled: Vec::new(),
                    }),
                    records,
                )
            },
            |m| m,
        )
    }

    pub(super) fn integration_started(
        &mut self,
        session: Session,
        records: Vec<CommandRecord>,
    ) -> Task<Message> {
        self.busy = None;
        let failed = records.last().filter(|r| !r.success()).cloned();
        for record in records {
            self.log(record);
        }
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let operation = repo::operation_in_progress(&repo.paths.git_dir);
        if operation.is_none()
            && let Some(record) = failed
        {
            // Refused before starting (e.g. local changes in the way).
            self.error =
                Some(t!("error.integrate_refused", detail = record.error_summary()).into_owned());
            return self.refresh(false);
        }
        repo.session = Some(session);
        if operation.is_none() {
            return self.finish_integration();
        }
        self.select_changes();
        self.refresh(false)
    }

    fn select_changes(&mut self) {
        if let Some(repo) = &mut self.repo {
            repo.section = Section::Changes;
        }
    }

    /// The operation is over: records it for undo and, after a rebase,
    /// looks for blocks kept twice.
    fn finish_integration(&mut self) -> Task<Message> {
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let Some(session) = repo.session.take() else {
            return self.refresh(false);
        };
        repo.resolver = None;
        let root = repo.paths.root.clone();
        let branch = repo
            .status
            .as_ref()
            .and_then(|s| s.branch.head.clone())
            .unwrap_or_else(|| "HEAD".to_string());
        self.notice = Some(t!("integrate.done", action = session.label).into_owned());
        Task::perform(
            async move {
                let (after, mut records) = git_undo::snapshot(&root).await;
                let check = if session.kind == Kind::Rebase {
                    let (check, r) = check_after_rebase(root, session.other.clone(), branch).await;
                    records.extend(r);
                    check
                } else {
                    None
                };
                let entry = Entry {
                    label: session.label,
                    moves: Moves::Integration,
                    before: session.before,
                    after,
                    file: None,
                    config: None,
                    refs: Vec::new(),
                    resolved: None,
                    inverse: None,
                };
                Message::IntegrationFinished(Box::new(entry), check.map(Box::new), records)
            },
            |m| m,
        )
    }

    pub(super) fn integration_finished(
        &mut self,
        entry: Entry,
        check: Option<AfterCheck>,
        records: Vec<CommandRecord>,
    ) -> Task<Message> {
        for record in records {
            self.log(record);
        }
        self.record_undo(entry);
        if let Some(repo) = &mut self.repo {
            repo.after_check = check;
        }
        self.refresh(false)
    }

    /// Reloads the conflict screen when the operation moved to a new step.
    pub(super) fn sync_resolver(&mut self) -> Task<Message> {
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        let Some(operation) = repo.operation else {
            repo.resolver = None;
            if repo.session.is_some() && !self.busy.is_some() {
                // Finished outside the app (terminal): nothing to undo here.
                repo.session = None;
            }
            return Task::none();
        };
        let Some(key) = resolver_key(repo) else {
            return Task::none();
        };
        if repo.resolver.as_ref().is_some_and(|r| r.key == key) || repo.resolver_loading {
            return Task::none();
        }
        repo.resolver_loading = true;
        let conflicted: Vec<String> = repo
            .status
            .as_ref()
            .map(|s| s.conflicts().map(|e| e.path.clone()).collect())
            .unwrap_or_default();
        let branch = repo.status.as_ref().and_then(|s| s.branch.head.clone());
        let root = repo.paths.root.clone();
        let git_dir = repo.paths.git_dir.clone();
        Task::perform(
            load_resolver(root.clone(), git_dir, operation, branch, conflicted, key),
            move |(resolver, records)| {
                Message::ResolverLoaded(root.clone(), Box::new(resolver), records)
            },
        )
    }

    pub(super) fn resolver_loaded(
        &mut self,
        root: PathBuf,
        mut resolver: Resolver,
        records: Vec<CommandRecord>,
    ) -> Task<Message> {
        for record in records {
            self.log(record);
        }
        let Some(repo) = self.repo.as_mut().filter(|r| r.paths.root == root) else {
            return Task::none();
        };
        repo.resolver_loading = false;
        // Keep the choices made on files still in conflict.
        if let Some(old) = repo.resolver.take() {
            for file in &mut resolver.files {
                if let Some(previous) = old
                    .files
                    .iter()
                    .find(|f| f.path == file.path && f.content == file.content)
                {
                    file.choices = previous.choices.clone();
                    file.show_base = previous.show_base;
                }
            }
            if old
                .selected
                .as_ref()
                .is_some_and(|s| resolver.files.iter().any(|f| f.path == *s))
            {
                resolver.selected = old.selected;
            }
        }
        repo.resolver = Some(resolver);
        Task::none()
    }

    fn resolver_file(&mut self, path: &str) -> Option<&mut ResolverFile> {
        self.repo
            .as_mut()?
            .resolver
            .as_mut()?
            .files
            .iter_mut()
            .find(|f| f.path == path)
    }

    pub(super) fn choose(
        &mut self,
        path: String,
        index: usize,
        choice: Option<Choice>,
    ) -> Task<Message> {
        if let Some(file) = self.resolver_file(&path) {
            if let Some(slot) = file.choices.get_mut(index) {
                *slot = choice;
            }
            file.editing = None;
            file.refresh_duplicates();
        }
        Task::none()
    }

    pub(super) fn start_editing(&mut self, path: String, index: usize) -> Task<Message> {
        if let Some(file) = self.resolver_file(&path) {
            let draft = match (file.parts.as_ref(), file.choices.get(index)) {
                (Some(parts), Some(choice)) => {
                    let conflict = parts
                        .iter()
                        .filter_map(|p| match p {
                            Part::Conflict(c) => Some(c),
                            Part::Text(_) => None,
                        })
                        .nth(index);
                    conflict.map(|c| c.resolve(choice.as_ref().unwrap_or(&Choice::Both)))
                }
                _ => None,
            };
            file.editing = Some((
                index,
                text_editor::Content::with_text(&draft.unwrap_or_default()),
            ));
        }
        Task::none()
    }

    pub(super) fn edit_conflict(
        &mut self,
        path: String,
        action: text_editor::Action,
    ) -> Task<Message> {
        if let Some((_, content)) = self.resolver_file(&path).and_then(|f| f.editing.as_mut()) {
            content.perform(action);
        }
        Task::none()
    }

    pub(super) fn apply_edit(&mut self, path: String) -> Task<Message> {
        if let Some(file) = self.resolver_file(&path)
            && let Some((index, content)) = file.editing.take()
        {
            let text = content.text();
            let text = text.strip_suffix('\n').unwrap_or(&text).to_string();
            if let Some(slot) = file.choices.get_mut(index) {
                *slot = Some(Choice::Custom(text));
            }
            file.refresh_duplicates();
        }
        Task::none()
    }

    /// Writes the settled file and marks it as settled (`git add`).
    pub(super) fn save_resolved(&mut self, path: String) -> Task<Message> {
        let Some(result) = self.resolver_file(&path).and_then(|f| f.result()) else {
            return Task::none();
        };
        self.write_and_add(
            path.clone(),
            result,
            true,
            t!("undo.resolve", path = path).into_owned(),
        )
    }

    /// Keeps one side's whole file (files that can't be split into
    /// conflicts).
    pub(super) fn keep_whole(&mut self, path: String, incoming: bool) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let Some(file) = repo
            .resolver
            .as_ref()
            .and_then(|r| r.files.iter().find(|f| f.path == path))
        else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let side_exists = !file.versions[if incoming { 2 } else { 1 }].is_empty();
        let commands = if side_exists {
            vec![
                GitCommand::new([
                    "checkout",
                    if incoming { "--theirs" } else { "--ours" },
                    "--",
                    path.as_str(),
                ])
                .in_dir(&root),
                integrate::mark_resolved(&root, &path),
            ]
        } else {
            // That side deleted the file.
            vec![GitCommand::new(["rm", "-q", "--", path.as_str()]).in_dir(&root)]
        };
        let mut undoable =
            Undoable::new(t!("undo.resolve", path = path).into_owned(), Moves::Index);
        undoable.watch_file = Some(root.join(&path));
        undoable.resolved = Some(path);
        self.perform(Action::Resolve, None, Some(undoable), commands)
    }

    /// Removes the stale copy of a block kept twice.
    pub(super) fn remove_duplicate(&mut self, path: String, index: usize) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        // During a merge the result is staged; after a rebase it is a new change.
        let (content, duplicate, stage) = if let Some(file) = repo
            .resolver
            .as_ref()
            .and_then(|r| r.files.iter().find(|f| f.path == path))
        {
            if file.conflicted {
                // Not written yet: the copy is removed when the file is saved.
                return Task::none();
            }
            (
                file.content.clone(),
                file.duplicates.get(index).cloned(),
                true,
            )
        } else if let Some((_, found)) = repo
            .after_check
            .as_ref()
            .and_then(|c| c.files.iter().find(|(p, _)| *p == path))
        {
            let content = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
            (content, found.get(index).cloned(), false)
        } else {
            return Task::none();
        };
        let Some(duplicate) = duplicate else {
            return Task::none();
        };
        let fixed = conflict::remove_copy(&content, &duplicate);
        if let Some(check) = self.repo.as_mut().and_then(|r| r.after_check.as_mut()) {
            check.files.retain(|(p, d)| *p != path || d.len() > 1);
            if let Some((_, d)) = check.files.iter_mut().find(|(p, _)| *p == path) {
                d.remove(index);
            }
        }
        self.write_and_add(
            path.clone(),
            fixed,
            stage,
            t!("undo.remove_duplicate", path = path).into_owned(),
        )
    }

    fn write_and_add(
        &mut self,
        path: String,
        content: String,
        add: bool,
        label: String,
    ) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let conflicted = repo
            .status
            .as_ref()
            .is_some_and(|s| s.conflicts().any(|e| e.path == path));
        let mut undoable = Undoable::new(label, Moves::Index);
        undoable.watch_file = Some(root.join(&path));
        if conflicted {
            undoable.resolved = Some(path.clone());
            if let Some(session) = self.repo.as_mut().and_then(|r| r.session.as_mut())
                && !session.settled.contains(&path)
            {
                session.settled.push(path.clone());
            }
        }
        let shell = format!("# write {}", crate::git::runner::quote(&path));
        self.perform_with(Action::Resolve, None, Some(undoable), async move {
            let result = tokio::fs::write(root.join(&path), content)
                .await
                .map_err(|e| e.to_string());
            let ok = result.is_ok();
            let mut records = vec![CommandRecord::local(shell, &root, result)];
            if ok && add {
                records.push(run(integrate::mark_resolved(&root, &path)).await);
            }
            records
        })
    }

    /// Goes on with the operation (saves the merge, continues the rebase).
    pub(super) fn proceed(&mut self) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let Some(operation) = repo.operation else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let git_dir = repo.paths.git_dir.clone();
        let command = integrate::proceed(&root, operation);
        self.busy = Some(t!("busy.proceeding").into_owned());
        Task::perform(
            async move {
                let record = run(command).await;
                let still = repo::operation_in_progress(&git_dir).is_some();
                Message::Proceeded(vec![record], still)
            },
            |m| m,
        )
    }

    pub(super) fn proceeded(&mut self, records: Vec<CommandRecord>, still: bool) -> Task<Message> {
        self.busy = None;
        let failed = records
            .iter()
            .find(|r| !r.success())
            .map(CommandRecord::error_summary);
        for record in records {
            self.log(record);
        }
        if !still {
            if let Some(repo) = &mut self.repo {
                repo.operation = None;
                repo.resolver = None;
            }
            if self.repo.as_ref().is_some_and(|r| r.session.is_some()) {
                return self.finish_integration();
            }
            self.notice = Some(t!("integrate.done_plain").into_owned());
        } else if let Some(detail) = failed {
            // A rebase stopping on the next commit is not an error.
            let conflicts = detail.contains("CONFLICT") || detail.contains("could not apply");
            if !conflicts {
                self.error = Some(t!("error.proceed_failed", detail = detail).into_owned());
            }
        }
        self.refresh(false)
    }

    // -------------------------------------------------------------- views

    pub(super) fn integration_prompt_view<'a>(
        &'a self,
        prompt: &'a Prompt,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let Some(repo) = &self.repo else {
            return Space::new().into();
        };
        let (current, dirty) = self.current_branch_and_changes();
        let current = current.unwrap_or_else(|| "HEAD".to_string());
        let other = prompt.other.as_str();
        let (title, intro, go) = match prompt.kind {
            Kind::Merge => (
                t!("integrate.merge_title", branch = other, into = current),
                t!("integrate.merge_intro", branch = other, into = current),
                t!("integrate.merge_button"),
            ),
            Kind::Rebase => (
                t!("integrate.rebase_title", branch = current, onto = other),
                t!("integrate.rebase_intro", branch = current, onto = other),
                t!("integrate.rebase_button"),
            ),
        };
        let command = integrate::start(
            &repo.paths.root,
            prompt.kind,
            other,
            prompt.after.as_deref(),
            prompt.ignore_whitespace,
        );
        let mut col = column![
            text(title).size(20).font(sans(Weight::Semibold)),
            text(intro).color(p.text_2),
        ]
        .spacing(14);
        if let Some(after) = &prompt.after {
            col = col.push(
                text(t!(
                    "drop.rebase_after_note",
                    commit = after.chars().take(7).collect::<String>()
                ))
                .size(13)
                .color(p.text_2),
            );
        }
        let published = repo
            .status
            .as_ref()
            .is_some_and(|s| s.branch.upstream.is_some());
        if prompt.kind == Kind::Rebase && published {
            col = col.push(
                container(text(t!("integrate.rebase_pushed", branch = current)).size(13))
                    .padding([10, 12])
                    .width(Length::Fill)
                    .style(style::banner_warning),
            );
        }
        col = col.push(
            checkbox(prompt.ignore_whitespace)
                .label(t!("integrate.ignore_whitespace"))
                .size(16)
                .text_size(13)
                .on_toggle(Message::SetPromptWhitespace)
                .style(style::check),
        );
        if dirty {
            col = col.push(text(t!("integrate.autostash")).size(13).color(p.text_2));
        }
        col = col
            .push(text(t!("integrate.steps")).size(13).color(p.text_2))
            .push(
                container(
                    row![
                        text("$").font(style::MONO).size(12).color(jc::PROMPT),
                        text(command.display()).font(style::MONO).size(12),
                    ]
                    .spacing(8),
                )
                .padding([8, 12])
                .width(Length::Fill)
                .style(style::journal_box),
            )
            .push(
                row![
                    text(t!("dialog.undoable")).size(12).color(p.muted),
                    space::horizontal(),
                    button(centered(text(t!("common.cancel"))))
                        .padding([0, 16])
                        .height(40)
                        .on_press(Message::ClosePrompt)
                        .style(style::secondary),
                    button(centered(text(go).font(sans(Weight::Semibold))))
                        .padding([0, 16])
                        .height(40)
                        .on_press_maybe(self.idle().then_some(Message::StartIntegration))
                        .style(style::primary),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            );
        container(col)
            .padding(24)
            .max_width(620)
            .style(style::dialog)
            .into()
    }

    /// The conflict screen, shown instead of the changes while an
    /// operation is in progress.
    pub(super) fn integration_view<'a>(
        &'a self,
        repo: &'a OpenRepo,
        operation: Operation,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let Some(resolver) = repo.resolver.as_ref().filter(|r| r.operation == operation) else {
            return container(text(t!("integrate.loading")).color(p.muted))
                .padding(16)
                .into();
        };
        let sides = &resolver.sides;
        let conflicts_left = repo.status.as_ref().map_or(0, |s| s.conflicts().count());
        let duplicates_left: usize = resolver
            .files
            .iter()
            .filter(|f| !f.conflicted)
            .map(|f| f.duplicates.len())
            .sum();

        // Dark header: what is going on and where we are.
        let current_tint = style::branch_tint(p, &sides.current);
        let incoming_tint = style::branch_tint(p, &sides.incoming);
        let what = match operation {
            Operation::Merge => row![
                text(sides.incoming.clone()).color(lighten(incoming_tint.strong)),
                text(t!("integrate.into")).color(jc::TEXT),
                text(sides.current.clone()).color(lighten(current_tint.strong)),
            ],
            Operation::Rebase => row![
                text(sides.incoming.clone()).color(lighten(incoming_tint.strong)),
                text(t!("integrate.onto")).color(jc::TEXT),
                text(sides.current.clone()).color(lighten(current_tint.strong)),
            ],
            Operation::CherryPick | Operation::Revert => row![
                text(
                    sides
                        .replaying
                        .clone()
                        .unwrap_or_else(|| sides.incoming.clone())
                )
                .color(jc::TEXT),
            ],
        }
        .spacing(6);
        let step = if conflicts_left > 0 { 1 } else { 2 };
        let mut steps = row![].spacing(6).align_y(Alignment::Center);
        for (i, key) in [
            "integrate.step_prepare",
            "integrate.step_resolve",
            "integrate.step_check",
            "integrate.step_save",
        ]
        .iter()
        .enumerate()
        {
            let label = if i < step {
                format!("✓ {}", tr(key))
            } else {
                format!("{} {}", i + 1, tr(key))
            };
            steps = steps.push(
                container(text(label).size(12))
                    .padding([3, 10])
                    .style(style::wizard_step(i.cmp(&step))),
            );
        }
        let header = container(
            row![
                column![
                    text(tr(operation.key()).to_uppercase())
                        .size(11)
                        .font(sans(Weight::Semibold))
                        .color(jc::HINT),
                    what,
                ]
                .spacing(2),
                steps,
                space::horizontal(),
            ]
            .spacing(18)
            .align_y(Alignment::Center),
        )
        .padding([12, 16])
        .width(Length::Fill)
        .style(style::journal);

        // Files on the left.
        let mut files = column![
            text(t!("integrate.files_title"))
                .size(13)
                .font(sans(Weight::Semibold))
                .color(p.text_2)
        ]
        .spacing(6);
        for file in &resolver.files {
            let selected = resolver.selected.as_deref() == Some(file.path.as_str());
            let still = repo
                .status
                .as_ref()
                .is_some_and(|s| s.conflicts().any(|e| e.path == file.path));
            let (state, color) = if file.conflicted && still {
                if file.parts.is_some() {
                    (
                        t!(
                            "integrate.file_conflicts",
                            total = file.total(),
                            settled = file.settled()
                        )
                        .into_owned(),
                        p.warning.text,
                    )
                } else {
                    (t!("integrate.file_whole").into_owned(), p.warning.text)
                }
            } else if file.conflicted
                || repo
                    .session
                    .as_ref()
                    .is_some_and(|s| s.settled.contains(&file.path))
            {
                (t!("integrate.file_settled").into_owned(), p.added.strong)
            } else if !file.duplicates.is_empty() {
                (
                    t!("integrate.file_duplicates", count = file.duplicates.len()).into_owned(),
                    p.warning.text,
                )
            } else {
                (t!("integrate.file_review").into_owned(), p.text_2)
            };
            files = files.push(
                button(
                    column![
                        text(file.path.clone()).size(13).font(mono(Weight::Medium)),
                        text(state).size(12).color(color),
                    ]
                    .spacing(2),
                )
                .padding([8, 10])
                .width(Length::Fill)
                .on_press(Message::SelectResolverFile(file.path.clone()))
                .style(style::choice(selected)),
            );
        }
        files = files.push(
            container(
                text(t!("integrate.false_conflicts_note"))
                    .size(12)
                    .color(p.text_2),
            )
            .padding([10, 12])
            .style(style::tinted(style::Tint {
                strong: p.border,
                soft: p.sidebar,
                text: p.text_2,
            })),
        );
        let files = scrollable(files.width(260))
            .style(style::scroll)
            .height(Length::Fill);

        // The selected file.
        let main: Element<'a, Message> = match resolver
            .selected
            .as_ref()
            .and_then(|s| resolver.files.iter().find(|f| f.path == *s))
        {
            Some(file) => self.resolver_file_view(repo, sides, operation, file, p),
            None => container(text(t!("integrate.no_files")).color(p.muted))
                .padding(16)
                .into(),
        };

        // Bottom bar.
        let abort_command = ops::abort(&repo.paths.root, operation).display();
        let remaining = if conflicts_left > 0 {
            t!("integrate.remaining", count = conflicts_left).into_owned()
        } else if duplicates_left > 0 {
            t!("integrate.remaining_duplicates", count = duplicates_left).into_owned()
        } else {
            t!("integrate.ready").into_owned()
        };
        let proceed_label = match operation {
            Operation::Merge => t!("integrate.save_merge"),
            Operation::Rebase => t!("integrate.continue_rebase"),
            Operation::CherryPick | Operation::Revert => t!("integrate.continue"),
        };
        let footer = row![
            button(centered(
                text(t!("integrate.abort")).color(p.removed.strong)
            ))
            .padding([0, 14])
            .height(40)
            .on_press_maybe(self.idle().then_some(Message::AbortOperation))
            .style(style::secondary),
            text(abort_command)
                .size(12)
                .font(style::MONO)
                .color(p.muted),
            space::horizontal(),
            text(remaining).size(13).color(if conflicts_left > 0 {
                p.warning.text
            } else {
                p.text_2
            }),
            button(centered(text(proceed_label).font(sans(Weight::Semibold))))
                .padding([0, 18])
                .height(40)
                .on_press_maybe((self.idle() && conflicts_left == 0).then_some(Message::Proceed))
                .style(style::primary),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        column![
            header,
            row![
                files,
                container(main).width(Length::Fill).height(Length::Fill)
            ]
            .spacing(16)
            .height(Length::Fill),
            footer,
        ]
        .spacing(12)
        .height(Length::Fill)
        .into()
    }

    fn resolver_file_view<'a>(
        &'a self,
        repo: &'a OpenRepo,
        sides: &'a Sides,
        operation: Operation,
        file: &'a ResolverFile,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let still = repo
            .status
            .as_ref()
            .is_some_and(|s| s.conflicts().any(|e| e.path == file.path));
        let (current_role, incoming_role) = match operation {
            Operation::Merge => (
                t!("integrate.role_current_merge"),
                t!("integrate.role_incoming_merge"),
            ),
            Operation::Rebase => (
                t!("integrate.role_current_rebase"),
                t!(
                    "integrate.role_incoming_rebase",
                    commit = sides.replaying.clone().unwrap_or_default()
                ),
            ),
            Operation::CherryPick => (
                t!("integrate.role_current_merge"),
                t!("integrate.role_incoming_pick"),
            ),
            Operation::Revert => (
                t!("integrate.role_current_merge"),
                t!("integrate.role_incoming_revert"),
            ),
        };
        let current_tint = style::branch_tint(p, &sides.current);
        let incoming_tint = style::branch_tint(p, &sides.incoming);
        let mut col = column![].spacing(16);

        if !file.conflicted || !still {
            // Merged by git, or already settled: check for duplicates.
            let mut card = column![
                container(
                    text(if file.conflicted {
                        t!("integrate.settled_title", path = file.path)
                    } else {
                        t!("integrate.review_title", path = file.path)
                    })
                    .size(15)
                    .font(sans(Weight::Semibold)),
                )
                .padding([12, 16]),
                hline(style::separator),
            ];
            if file.duplicates.is_empty() {
                card = card.push(
                    container(text(t!("integrate.no_duplicates")).color(p.text_2)).padding(16),
                );
            }
            for (i, duplicate) in file.duplicates.iter().enumerate() {
                card = card.push(self.duplicate_box(&file.path, i, duplicate, sides, true, p));
            }
            col = col.push(container(card).width(Length::Fill).style(style::card));
            return scrollable(col)
                .style(style::scroll)
                .height(Length::Fill)
                .into();
        }

        let Some(parts) = &file.parts else {
            // Whole-file choice.
            let keep = |incoming: bool, name: &str, tint: style::Tint| {
                button(centered(
                    text(t!("integrate.keep_whole", branch = name))
                        .font(sans(Weight::Semibold))
                        .color(tint.text),
                ))
                .padding([0, 14])
                .height(36)
                .on_press_maybe(
                    self.idle()
                        .then(|| Message::KeepWhole(file.path.clone(), incoming)),
                )
                .style(style::secondary)
            };
            let card = column![
                text(t!("integrate.whole_title", path = file.path))
                    .size(15)
                    .font(sans(Weight::Semibold)),
                text(t!("integrate.whole_intro")).color(p.text_2),
                row![
                    keep(false, &sides.current, current_tint),
                    keep(true, &sides.incoming, incoming_tint),
                ]
                .spacing(10),
            ]
            .spacing(12);
            return container(card)
                .padding(16)
                .width(Length::Fill)
                .style(style::card)
                .into();
        };

        let conflicts: Vec<&conflict::Conflict> = parts
            .iter()
            .filter_map(|part| match part {
                Part::Conflict(c) => Some(c),
                Part::Text(_) => None,
            })
            .collect();
        let total = conflicts.len();
        for (i, c) in conflicts.into_iter().enumerate() {
            let choice = file.choices.get(i).cloned().flatten();
            let side = |name: &'a str,
                        role: String,
                        tint: style::Tint,
                        body: &'a str,
                        picked: bool,
                        which: Choice| {
                column![
                    container(
                        row![
                            container(Space::new())
                                .width(10)
                                .height(10)
                                .style(style::dot(tint.strong)),
                            text(name.to_string())
                                .font(sans(Weight::Semibold))
                                .color(tint.text),
                            text(role).size(12).color(p.text_2),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    )
                    .padding([0, 14])
                    .width(Length::Fill)
                    .center_y(52)
                    .style(style::side_header(tint)),
                    container(code_block(body, &file.path, p))
                        .padding([10, 14])
                        .width(Length::Fill),
                    container(
                        button(centered(
                            text(if picked {
                                format!("✓ {}", t!("integrate.keep_side", branch = name))
                            } else {
                                t!("integrate.keep_side", branch = name).into_owned()
                            })
                            .size(13)
                            .font(sans(Weight::Semibold))
                            .color(if picked {
                                style::text_on(tint.strong)
                            } else {
                                tint.text
                            }),
                        ))
                        .padding([0, 12])
                        .height(32)
                        .on_press(Message::Choose(
                            file.path.clone(),
                            i,
                            if picked { None } else { Some(which) }
                        ))
                        .style(style::side_button(tint, picked)),
                    )
                    .padding(Padding::default().left(14).right(14).bottom(12)),
                ]
                .width(Length::Fill)
            };
            let both_picked = choice == Some(Choice::Both);
            let custom_picked = matches!(choice, Some(Choice::Custom(_)));
            let mut card = column![
                container(
                    text(t!(
                        "integrate.conflict_title",
                        index = i + 1,
                        total = total,
                        path = file.path
                    ))
                    .size(15)
                    .font(sans(Weight::Semibold)),
                )
                .padding([12, 16]),
                hline(style::separator),
                row![
                    side(
                        &sides.current,
                        current_role.to_string(),
                        current_tint,
                        &c.current,
                        choice == Some(Choice::Current),
                        Choice::Current
                    ),
                    container(Space::new())
                        .width(1)
                        .height(Length::Fill)
                        .style(style::separator),
                    side(
                        &sides.incoming,
                        incoming_role.to_string(),
                        incoming_tint,
                        &c.incoming,
                        choice == Some(Choice::Incoming),
                        Choice::Incoming
                    ),
                ]
                .height(Length::Shrink),
            ];
            if file.show_base
                && let Some(base) = &c.base
            {
                card = card.push(hline(style::separator)).push(
                    column![
                        text(t!("integrate.base_title"))
                            .size(12)
                            .font(sans(Weight::Semibold))
                            .color(p.text_2),
                        code_block(base, &file.path, p),
                    ]
                    .spacing(6)
                    .padding([10, 16]),
                );
            }
            if let Some((index, content)) = file.editing.as_ref().filter(|(idx, _)| *idx == i) {
                let _ = index;
                let path = file.path.clone();
                card = card.push(hline(style::separator)).push(
                    column![
                        text_editor(content)
                            .height(160)
                            .padding(10)
                            .size(13)
                            .font(style::MONO)
                            .on_action(move |a| Message::EditConflict(path.clone(), a))
                            .style(style::editor),
                        row![
                            space::horizontal(),
                            button(centered(text(t!("common.cancel")).size(13)))
                                .padding([0, 12])
                                .height(32)
                                .on_press(Message::Choose(file.path.clone(), i, choice.clone()))
                                .style(style::secondary),
                            button(centered(
                                text(t!("integrate.use_text"))
                                    .size(13)
                                    .font(sans(Weight::Semibold))
                            ))
                            .padding([0, 12])
                            .height(32)
                            .on_press(Message::ApplyConflictEdit(file.path.clone()))
                            .style(style::primary),
                        ]
                        .spacing(8),
                    ]
                    .spacing(8)
                    .padding([10, 16]),
                );
            }
            let mut actions = row![
                button(centered(
                    text(t!(
                        "integrate.keep_both",
                        first = sides.current,
                        second = sides.incoming
                    ))
                    .size(13)
                ))
                .padding([0, 12])
                .height(32)
                .on_press(Message::Choose(
                    file.path.clone(),
                    i,
                    if both_picked {
                        None
                    } else {
                        Some(Choice::Both)
                    }
                ))
                .style(style::choice(both_picked)),
                button(centered(text(t!("integrate.write_myself")).size(13)))
                    .padding([0, 12])
                    .height(32)
                    .on_press(Message::StartConflictEdit(file.path.clone(), i))
                    .style(style::choice(custom_picked)),
                space::horizontal(),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if c.base.is_some() {
                actions = actions.push(
                    button(
                        text(if file.show_base {
                            t!("integrate.hide_base")
                        } else {
                            t!("integrate.show_base")
                        })
                        .size(13),
                    )
                    .padding([4, 6])
                    .on_press(Message::ToggleBase(file.path.clone()))
                    .style(style::link),
                );
            }
            card = card.push(
                container(actions)
                    .padding([10, 16])
                    .width(Length::Fill)
                    .style(style::card_footer),
            );
            col = col.push(container(card).width(Length::Fill).style(style::card));
        }

        // The result and its check.
        let mut result = column![
            container(
                row![
                    text(t!("integrate.result_title", path = file.path))
                        .size(15)
                        .font(sans(Weight::Semibold))
                        .width(Length::Fill),
                    button(centered(
                        text(t!("integrate.save_file")).font(sans(Weight::Semibold))
                    ))
                    .padding([0, 14])
                    .height(36)
                    .on_press_maybe(
                        (self.idle() && file.result().is_some())
                            .then(|| Message::SaveResolved(file.path.clone())),
                    )
                    .style(style::primary),
                ]
                .align_y(Alignment::Center),
            )
            .padding([10, 16]),
            hline(style::separator),
        ];
        match file.result() {
            Some(_) if file.duplicates.is_empty() => {
                result = result
                    .push(container(text(t!("integrate.result_ok")).color(p.text_2)).padding(16));
            }
            Some(_) => {
                for (i, duplicate) in file.duplicates.iter().enumerate() {
                    result =
                        result.push(self.duplicate_box(&file.path, i, duplicate, sides, false, p));
                }
            }
            None => {
                result = result.push(
                    container(
                        text(t!(
                            "integrate.result_pending",
                            count = file.total() - file.settled()
                        ))
                        .color(p.muted),
                    )
                    .padding(16),
                );
            }
        }
        col = col.push(container(result).width(Length::Fill).style(style::card));
        scrollable(col.padding(Padding::default().right(12)))
            .style(style::scroll)
            .height(Length::Fill)
            .into()
    }

    fn duplicate_box<'a>(
        &'a self,
        path: &str,
        index: usize,
        duplicate: &'a Duplicate,
        sides: &'a Sides,
        fixable: bool,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let first = duplicate
            .lines
            .first()
            .map(|l| l.trim().to_string())
            .unwrap_or_default();
        let mut line = row![
            icon(Icon::Warning, 18.0, p.warning.strong),
            column![
                text(t!(
                    "integrate.duplicate",
                    block = first,
                    count = duplicate.lines.len(),
                    a = duplicate.first + 1,
                    b = duplicate.second + 1
                ))
                .color(p.warning.text),
                text(t!(
                    "integrate.duplicate_hint",
                    current = sides.current,
                    incoming = sides.incoming,
                    line = duplicate.stale + 1
                ))
                .size(12)
                .color(p.text_2),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if fixable {
            line = line.push(
                button(centered(
                    text(t!("integrate.keep_one_copy"))
                        .font(sans(Weight::Semibold))
                        .color(Color::WHITE),
                ))
                .padding([0, 12])
                .height(32)
                .on_press_maybe(
                    self.idle()
                        .then(|| Message::RemoveDuplicate(path.to_string(), index)),
                )
                .style(style::warning_button),
            );
        }
        container(line)
            .padding([10, 14])
            .width(Length::Fill)
            .style(style::banner_warning)
            .into()
    }

    /// After a rebase: the blocks kept twice, with a fix for each.
    pub(super) fn after_check_view<'a>(
        &'a self,
        check: &'a AfterCheck,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let mut col = column![
            row![
                text(t!("integrate.after_title"))
                    .font(sans(Weight::Semibold))
                    .width(Length::Fill),
                button(text(t!("common.ok")).font(sans(Weight::Semibold)))
                    .on_press(Message::DismissAfterCheck)
                    .style(style::link),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(8);
        for (path, found) in &check.files {
            col = col.push(text(path.clone()).size(13).font(mono(Weight::Medium)));
            for (i, d) in found.iter().enumerate() {
                col = col.push(self.duplicate_box(path, i, d, &check.sides, true, p));
            }
        }
        container(col)
            .padding([10, 12])
            .width(Length::Fill)
            .style(style::card)
            .into()
    }

    /// The current branch, and whether there are local changes.
    pub(super) fn current_branch_and_changes(&self) -> (Option<String>, bool) {
        let Some(status) = self.repo.as_ref().and_then(|r| r.status.as_ref()) else {
            return (None, false);
        };
        let dirty = status.entries.iter().any(|e| e.kind != EntryKind::Ignored);
        (status.branch.head.clone(), dirty)
    }
}

/// Lines in the monospace font, as in the file, with the colors of its
/// language when it is known.
fn code_block<'a>(body: &'a str, path: &str, p: &'static Palette) -> Element<'a, Message> {
    if body.is_empty() {
        return text(t!("integrate.empty_side"))
            .font(style::MONO)
            .size(13)
            .color(p.muted)
            .into();
    }
    let Some(lang) = syntax::language_for(path) else {
        return text(body.trim_end_matches('\n'))
            .font(style::MONO)
            .size(13)
            .color(p.text)
            .into();
    };
    let mut state = syntax::State::Normal;
    let mut col = column![];
    for line in body.trim_end_matches('\n').lines() {
        let pieces = syntax::highlight(lang, line, &mut state);
        col = col.push(super::changes::code(line, &pieces, p.text, p));
    }
    col.into()
}

/// A branch color made readable on the dark header.
fn lighten(c: Color) -> Color {
    Color {
        r: c.r + (1.0 - c.r) * 0.45,
        g: c.g + (1.0 - c.g) * 0.45,
        b: c.b + (1.0 - c.b) * 0.45,
        a: 1.0,
    }
}
