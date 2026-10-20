//! Drag and drop in the history: a commit or a branch label is dropped on
//! another commit or label, and the gesture is turned into one git action
//! (rebase, merge, cherry-pick, move a branch, push, pull, swap or squash two
//! commits), always confirmed with the exact command first.

use std::collections::{HashMap, HashSet};

use iced::Point;

use super::history::{Dialog, is_on_remote, local_name};
use super::undo::Undoable;
use super::*;
use crate::git::branches;
use crate::git::integrate::Kind;
use crate::git::rewrite::{self, Edit};
use crate::git::undo::Moves;

/// The pointer moves this far before a press becomes a drag.
const DRAG_THRESHOLD: f32 = 6.0;

/// What is dragged, or what is under the pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grip {
    Commit(String),
    /// A branch label, local (`main`) or remote (`origin/main`).
    Branch(String),
    /// The Workspaces item of the sidebar (only a drop target).
    Workspaces,
}

#[derive(Debug, Clone)]
pub struct Drag {
    pub grip: Grip,
    pub from: Point,
    /// The pointer went far enough: this is a drag, not a click.
    pub moving: bool,
}

/// What a drop does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropPlan {
    /// Opens the merge or rebase assistant (`after`: rebase `--onto`).
    Integrate {
        kind: Kind,
        other: String,
        after: Option<String>,
        /// The commit dropped, for the label.
        moved: Option<String>,
    },
    CherryPick {
        commit: String,
        onto: String,
    },
    MoveBranch {
        branch: String,
        to: String,
        current: bool,
    },
    Push {
        local: String,
        remote: String,
    },
    /// Brings a server branch into its local branch.
    Pull {
        local: String,
        remote: String,
        current: bool,
    },
    /// Two neighbouring commits of the current branch, not sent yet.
    Rewrite {
        older: String,
        newer: String,
    },
    /// Opens the "New workspace" form on a branch or commit.
    NewWorkspace(crate::git::worktrees::Start),
    /// The gesture means nothing here; the text says why.
    Refused(String),
}

fn short(hash: &str) -> String {
    hash.chars().take(7).collect()
}

/// The commits of the current branch, newest first, following first parents.
fn first_parent_chain(repo: &OpenRepo) -> Vec<String> {
    let by_hash: HashMap<&str, &Commit> =
        repo.commits.iter().map(|c| (c.hash.as_str(), c)).collect();
    let mut chain = Vec::new();
    let mut current = repo.status.as_ref().and_then(|s| s.branch.oid.clone());
    while let Some(hash) = current {
        current = by_hash
            .get(hash.as_str())
            .and_then(|c| c.parents.first().cloned());
        chain.push(hash);
    }
    chain
}

/// Whether `ancestor` can be reached from `from` through parents.
fn is_ancestor(repo: &OpenRepo, ancestor: &str, from: &str) -> bool {
    let by_hash: HashMap<&str, &Commit> =
        repo.commits.iter().map(|c| (c.hash.as_str(), c)).collect();
    let mut stack = vec![from];
    let mut seen = HashSet::new();
    while let Some(hash) = stack.pop() {
        if hash == ancestor {
            return true;
        }
        if seen.insert(hash)
            && let Some(c) = by_hash.get(hash)
        {
            stack.extend(c.parents.iter().map(String::as_str));
        }
    }
    false
}

fn commit<'a>(repo: &'a OpenRepo, hash: &str) -> Option<&'a Commit> {
    repo.commits.iter().find(|c| c.hash == hash)
}

/// The commit a branch label points at.
fn tip<'a>(repo: &'a OpenRepo, branch: &str) -> Option<&'a str> {
    repo.commits
        .iter()
        .find(|c| c.refs.iter().any(|r| r == branch))
        .map(|c| c.hash.as_str())
}

fn is_remote(repo: &OpenRepo, name: &str) -> bool {
    repo.branches.iter().any(|b| b.remote && b.name == name)
}

/// A name for a commit in a command: a branch pointing at it, or its id.
fn name_of(repo: &OpenRepo, hash: &str, current: Option<&str>) -> String {
    let refs = commit(repo, hash).map(|c| c.refs.as_slice()).unwrap_or(&[]);
    let branch = |remote: bool| {
        refs.iter().find(|r| {
            Some(r.as_str()) != current
                && !r.starts_with("tag: ")
                && *r != "HEAD"
                && !r.ends_with("/HEAD")
                && is_remote(repo, r) == remote
        })
    };
    branch(false)
        .or_else(|| branch(true))
        .cloned()
        .unwrap_or_else(|| short(hash))
}

impl App {
    pub(super) fn grab(&mut self, grip: Grip) -> Task<Message> {
        let pointer = self.pointer;
        let select = match &grip {
            Grip::Commit(hash) => Some(hash.clone()),
            Grip::Branch(name) => self
                .repo
                .as_ref()
                .and_then(|r| tip(r, name))
                .map(String::from),
            Grip::Workspaces => return Task::none(),
        };
        let Some(repo) = &mut self.repo else {
            return Task::none();
        };
        repo.history.drag = Some(Drag {
            grip,
            from: pointer,
            moving: false,
        });
        match select {
            Some(hash) => self.select_commit(hash),
            None => Task::none(),
        }
    }

    pub(super) fn hover(&mut self, grip: Option<Grip>) {
        if let Some(repo) = &mut self.repo {
            repo.history.hover = grip;
        }
    }

    /// Called as the pointer moves: a press that travels becomes a drag.
    pub(super) fn drag_moved(&mut self, point: Point) {
        if let Some(drag) = self.repo.as_mut().and_then(|r| r.history.drag.as_mut())
            && !drag.moving
            && drag.from.distance(point) > DRAG_THRESHOLD
        {
            drag.moving = true;
        }
    }

    pub(super) fn dragging(&self) -> Option<&Drag> {
        self.repo
            .as_ref()
            .and_then(|r| r.history.drag.as_ref())
            .filter(|d| d.moving)
    }

    /// The plan of a drop where the pointer is now, if it is over something.
    pub(super) fn current_plan(&self) -> Option<DropPlan> {
        let drag = self.dragging()?;
        let target = self.repo.as_ref()?.history.hover.as_ref()?;
        self.plan(&drag.grip, target, self.modifiers.command())
    }

    pub(super) fn release(&mut self) -> Task<Message> {
        let plan = self.current_plan();
        if let Some(repo) = &mut self.repo {
            repo.history.drag = None;
        }
        match plan {
            None => Task::none(),
            Some(DropPlan::Refused(reason)) => {
                self.notice = Some(reason);
                Task::none()
            }
            Some(DropPlan::NewWorkspace(start)) => self.new_workspace(Some(start)),
            Some(DropPlan::Integrate {
                kind, other, after, ..
            }) => {
                let task = self.open_integration(kind, other);
                if let Some(prompt) = &mut self.integration_prompt {
                    prompt.after = after;
                }
                task
            }
            // The current branch is sent with the usual wizard, which gets
            // colleagues' work first.
            Some(DropPlan::Push { local, .. })
                if self
                    .repo
                    .as_ref()
                    .and_then(|r| r.status.as_ref())
                    .and_then(|s| s.branch.head.as_deref())
                    == Some(local.as_str()) =>
            {
                self.update(Message::OpenSend)
            }
            Some(plan) => {
                self.history_dialog = Some(Dialog::Drop {
                    plan,
                    squash: false,
                });
                Task::none()
            }
        }
    }

    /// What dropping `grip` on `target` does.
    pub(super) fn plan(&self, grip: &Grip, target: &Grip, ctrl: bool) -> Option<DropPlan> {
        let repo = self.repo.as_ref()?;
        if grip == target || !self.idle() {
            return None;
        }
        // Dropped on the Workspaces item: open it in a new workspace.
        if *target == Grip::Workspaces {
            use crate::git::worktrees::Start;
            let start = match grip {
                Grip::Commit(hash) => Start::Detached(hash.clone()),
                Grip::Branch(name) if is_remote(repo, name) => {
                    let local = local_name(repo, name);
                    if repo.branches.iter().any(|b| !b.remote && b.name == local) {
                        Start::Branch(local)
                    } else {
                        Start::NewBranch {
                            name: local,
                            at: name.clone(),
                        }
                    }
                }
                Grip::Branch(name) => Start::Branch(name.clone()),
                Grip::Workspaces => return None,
            };
            return Some(DropPlan::NewWorkspace(start));
        }
        if repo.operation.is_some() {
            return Some(DropPlan::Refused(t!("drop.busy").into_owned()));
        }
        let current = repo.status.as_ref().and_then(|s| s.branch.head.clone());
        let current = current.as_deref();

        // A label on its own counterpart on the server: send or get.
        if let (Grip::Branch(from), Grip::Branch(to)) = (grip, target) {
            let (from_remote, to_remote) = (is_remote(repo, from), is_remote(repo, to));
            if !from_remote && to_remote && local_name(repo, to) == *from {
                return Some(DropPlan::Push {
                    local: from.clone(),
                    remote: to.clone(),
                });
            }
            if from_remote && !to_remote && local_name(repo, from) == *to {
                return Some(DropPlan::Pull {
                    local: to.clone(),
                    remote: from.clone(),
                    current: current == Some(to.as_str()),
                });
            }
        }

        let (target_hash, target_branch) = match target {
            Grip::Commit(hash) => (hash.clone(), None),
            Grip::Branch(name) => (tip(repo, name)?.to_string(), Some(name.clone())),
            Grip::Workspaces => return None,
        };

        match grip {
            Grip::Branch(branch) => {
                if is_remote(repo, branch) {
                    return Some(DropPlan::Refused(
                        t!("drop.remote_moves", branch = branch).into_owned(),
                    ));
                }
                if tip(repo, branch) == Some(target_hash.as_str()) {
                    return None;
                }
                Some(DropPlan::MoveBranch {
                    branch: branch.clone(),
                    to: target_hash,
                    current: current == Some(branch.as_str()),
                })
            }
            Grip::Commit(source) => {
                if *source == target_hash {
                    return None;
                }
                let head = repo.status.as_ref().and_then(|s| s.branch.oid.clone());
                // The branch the drop lands on: the label, or the current
                // branch when its last commit is the target.
                let landing = target_branch.clone().or_else(|| {
                    (head.as_deref() == Some(target_hash.as_str()))
                        .then(|| current.map(String::from))
                        .flatten()
                });
                let lands_on_current = landing.is_some() && landing.as_deref() == current;
                let lands_on_head = head.as_deref() == Some(target_hash.as_str());

                if ctrl {
                    return Some(match (current, lands_on_current || lands_on_head) {
                        (Some(onto), true) => DropPlan::CherryPick {
                            commit: source.clone(),
                            onto: onto.to_string(),
                        },
                        _ => DropPlan::Refused(
                            t!(
                                "drop.cherry_pick_elsewhere",
                                branch = landing.unwrap_or_else(|| short(&target_hash))
                            )
                            .into_owned(),
                        ),
                    });
                }

                let chain = first_parent_chain(repo);
                let position = |h: &str| chain.iter().position(|c| c == h);
                match (position(source), position(&target_hash)) {
                    // Two commits of the current branch: neighbours swap or squash.
                    (Some(a), Some(b)) => {
                        if a.abs_diff(b) != 1 {
                            return Some(DropPlan::Refused(t!("drop.not_neighbours").into_owned()));
                        }
                        let (newer, older) = if a < b {
                            (source.clone(), target_hash)
                        } else {
                            (target_hash, source.clone())
                        };
                        let linear = chain[..=a.max(b)]
                            .iter()
                            .all(|h| commit(repo, h).is_some_and(|c| c.parents.len() <= 1));
                        if !linear {
                            return Some(DropPlan::Refused(t!("drop.merge_inside").into_owned()));
                        }
                        if is_on_remote(repo, &older) {
                            return Some(DropPlan::Refused(t!("drop.already_sent").into_owned()));
                        }
                        Some(DropPlan::Rewrite { older, newer })
                    }
                    // A commit of the current branch dropped elsewhere: the
                    // branch from that commit on is replayed there.
                    (Some(_), None) => {
                        let parent = commit(repo, source)?.parents.first()?.clone();
                        let other = target_branch
                            .clone()
                            .unwrap_or_else(|| name_of(repo, &target_hash, current));
                        // From the branch's first own commit, a plain rebase
                        // says the same thing.
                        let after = (!is_ancestor(repo, &parent, &target_hash)).then_some(parent);
                        Some(DropPlan::Integrate {
                            kind: Kind::Rebase,
                            other,
                            after,
                            moved: Some(source.clone()),
                        })
                    }
                    // Another commit dropped on the current branch: merge it.
                    (None, _) if lands_on_current || lands_on_head => Some(DropPlan::Integrate {
                        kind: Kind::Merge,
                        other: name_of(repo, source, current),
                        after: None,
                        moved: None,
                    }),
                    (None, _) => Some(DropPlan::Refused(
                        t!(
                            "drop.go_there_first",
                            branch = landing.unwrap_or_else(|| short(&target_hash))
                        )
                        .into_owned(),
                    )),
                }
            }
            Grip::Workspaces => None,
        }
    }

    /// One line describing a plan, shown next to the pointer while dragging.
    pub(super) fn plan_label(&self, plan: &DropPlan) -> String {
        let current = self
            .repo
            .as_ref()
            .and_then(|r| r.status.as_ref())
            .and_then(|s| s.branch.head.clone())
            .unwrap_or_else(|| "HEAD".into());
        match plan {
            DropPlan::Integrate {
                kind: Kind::Merge,
                other,
                ..
            } => t!("drop.label_merge", branch = other, into = current),
            DropPlan::Integrate {
                kind: Kind::Rebase,
                other,
                after: Some(_),
                moved,
            } => t!(
                "drop.label_rebase_from",
                commit = moved.as_deref().map(short).unwrap_or_default(),
                onto = other
            ),
            DropPlan::Integrate { other, .. } => {
                t!("drop.label_rebase", branch = current, onto = other)
            }
            DropPlan::CherryPick { commit, onto } => {
                t!(
                    "drop.label_cherry_pick",
                    commit = short(commit),
                    onto = onto
                )
            }
            DropPlan::MoveBranch { branch, to, .. } => {
                t!("drop.label_move", branch = branch, commit = short(to))
            }
            DropPlan::Push { local, remote } => {
                t!("drop.label_push", branch = local, remote = remote)
            }
            DropPlan::Pull { local, remote, .. } => {
                t!("drop.label_pull", branch = local, remote = remote)
            }
            DropPlan::Rewrite { .. } => t!("drop.label_rewrite"),
            DropPlan::NewWorkspace(start) => {
                use crate::git::worktrees::Start;
                match start {
                    Start::Branch(branch) => t!("drop.label_workspace", what = branch),
                    Start::NewBranch { name, .. } => t!("drop.label_workspace", what = name),
                    Start::Detached(commit) => {
                        t!("drop.label_workspace_commit", commit = short(commit))
                    }
                }
            }
            DropPlan::Refused(reason) => reason.as_str().into(),
        }
        .into_owned()
    }

    /// The edit, base and replayed commits of a swap or squash.
    pub(super) fn rewrite_parts(&self, edit: &Edit) -> Option<(Option<String>, Vec<String>)> {
        let repo = self.repo.as_ref()?;
        let mut chain = first_parent_chain(repo);
        let oldest = chain.iter().position(|h| h == edit.oldest())?;
        chain.truncate(oldest + 1);
        chain.reverse();
        let base = commit(repo, edit.oldest())?.parents.first().cloned();
        Some((base, chain))
    }

    pub(super) fn rewrite_edit(older: &str, newer: &str, squash: bool) -> Edit {
        let (older, newer) = (older.to_string(), newer.to_string());
        if squash {
            Edit::Squash { older, newer }
        } else {
            Edit::Swap { older, newer }
        }
    }

    /// The commands a confirmed plan runs, as shown in its dialog.
    pub(super) fn plan_commands(&self, plan: &DropPlan, squash: bool) -> Vec<String> {
        let Some(repo) = &self.repo else {
            return Vec::new();
        };
        let root = &repo.paths.root;
        match plan {
            DropPlan::CherryPick { commit: c, .. } => {
                let merge = commit(repo, c).is_some_and(|c| c.parents.len() > 1);
                vec![branches::cherry_pick(root, &short(c), merge).display()]
            }
            DropPlan::MoveBranch {
                branch,
                to,
                current,
            } => branches::move_branch(root, branch, &short(to), *current)
                .iter()
                .map(GitCommand::display)
                .collect(),
            DropPlan::Push { local, remote } => {
                vec![branches::push_branch(root, local, remote).display()]
            }
            DropPlan::Pull {
                local,
                remote,
                current: false,
            } => vec![branches::fast_forward_branch(root, local, remote).display()],
            DropPlan::Pull { .. } => vec!["git fetch".into(), "git rebase / git merge".into()],
            DropPlan::Rewrite { older, newer } => {
                let edit = Self::rewrite_edit(older, newer, squash);
                let Some((base, chain)) = self.rewrite_parts(&edit) else {
                    return Vec::new();
                };
                let mut lines = vec![rewrite::shown(root, base.as_deref().map(short).as_deref())];
                if let Some(todo) = rewrite::todo(&chain, &edit, "") {
                    lines.extend(todo.lines().map(|l| {
                        let (verb, hash) = l.split_once(' ').unwrap_or((l, ""));
                        format!("  {verb} {}", short(hash))
                    }));
                }
                lines
            }
            DropPlan::Integrate { .. } | DropPlan::NewWorkspace(_) | DropPlan::Refused(_) => {
                Vec::new()
            }
        }
    }

    /// Runs a plan confirmed in its dialog.
    pub(super) fn run_plan(&mut self, plan: DropPlan, squash: bool) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        match plan {
            DropPlan::CherryPick { commit, .. } => {
                self.menu_action(super::history::MenuAction::CherryPick(commit))
            }
            DropPlan::MoveBranch {
                branch,
                to,
                current,
            } => {
                let commands = branches::move_branch(&root, &branch, &to, current);
                let mut undoable = Undoable::new(
                    t!("undo.move_branch", branch = branch, commit = short(&to)).into_owned(),
                    if current {
                        Moves::Integration
                    } else {
                        Moves::Index
                    },
                );
                undoable.refs = vec![
                    format!("refs/heads/{branch}"),
                    branches::backup_ref(&branch),
                ];
                self.perform(Action::History, None, Some(undoable), commands)
            }
            DropPlan::Push { local, remote } => {
                let command = branches::push_branch(&root, &local, &remote);
                self.perform(
                    Action::Push,
                    Some(t!("busy.sending").into_owned()),
                    None,
                    vec![command],
                )
            }
            DropPlan::Pull { current: true, .. } => self.update(Message::Fetch),
            DropPlan::Pull { local, remote, .. } => {
                let command = branches::fast_forward_branch(&root, &local, &remote);
                let mut undoable = Undoable::new(
                    t!("undo.pull_branch", branch = local).into_owned(),
                    Moves::Index,
                );
                undoable.refs = vec![format!("refs/heads/{local}")];
                self.perform(
                    Action::History,
                    Some(t!("busy.fetching").into_owned()),
                    Some(undoable),
                    vec![command],
                )
            }
            DropPlan::Rewrite { older, newer } => {
                let edit = Self::rewrite_edit(&older, &newer, squash);
                let label = if squash {
                    t!("undo.squash", commit = short(&newer), into = short(&older))
                } else {
                    t!("undo.swap", first = short(&newer), second = short(&older))
                }
                .into_owned();
                self.rewrite(edit, label)
            }
            DropPlan::Integrate { .. } | DropPlan::NewWorkspace(_) | DropPlan::Refused(_) => {
                Task::none()
            }
        }
    }

    /// Runs an interactive rebase prepared by the app.
    pub(super) fn rewrite(&mut self, edit: Edit, label: String) -> Task<Message> {
        let Some((base, chain)) = self.rewrite_parts(&edit) else {
            return Task::none();
        };
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        let git_dir = repo.paths.git_dir.clone();
        self.perform_with(
            Action::Rewrite,
            Some(t!("busy.rewriting").into_owned()),
            Some(Undoable::new(label, Moves::Integration)),
            async move { rewrite::apply(&root, &git_dir, base.as_deref(), &chain, &edit).await },
        )
    }

    /// Whether the message of `hash` can be edited: a commit of the current
    /// branch with no merge between it and the last one.
    pub(super) fn can_reword(&self, hash: &str) -> bool {
        let Some(repo) = &self.repo else {
            return false;
        };
        let chain = first_parent_chain(repo);
        chain.iter().position(|h| h == hash).is_some_and(|at| {
            chain[..=at]
                .iter()
                .all(|h| commit(repo, h).is_some_and(|c| c.parents.len() <= 1))
        })
    }

    /// Title, explanation and button of a drop's confirmation.
    pub(super) fn drop_texts(&self, plan: &DropPlan, squash: bool) -> (String, String, String) {
        let subject = |hash: &str| {
            self.repo
                .as_ref()
                .and_then(|r| commit(r, hash))
                .map(|c| c.subject.clone())
                .unwrap_or_default()
        };
        let (title, intro, confirm) = match plan {
            DropPlan::CherryPick { commit, onto } => (
                t!(
                    "drop.label_cherry_pick",
                    commit = short(commit),
                    onto = onto
                ),
                t!(
                    "drop.cherry_pick_intro",
                    subject = subject(commit),
                    onto = onto
                ),
                t!("drop.cherry_pick_button"),
            ),
            DropPlan::MoveBranch {
                branch,
                to,
                current,
            } => (
                t!("drop.label_move", branch = branch, commit = short(to)),
                if *current {
                    t!(
                        "drop.move_current_intro",
                        branch = branch,
                        subject = subject(to)
                    )
                } else {
                    t!("drop.move_intro", branch = branch, subject = subject(to))
                },
                t!("drop.move_button"),
            ),
            DropPlan::Push { local, remote } => (
                t!("drop.label_push", branch = local, remote = remote),
                t!("drop.push_intro", branch = local, remote = remote),
                t!("drop.push_button"),
            ),
            DropPlan::Pull { local, remote, .. } => (
                t!("drop.label_pull", branch = local, remote = remote),
                t!("drop.pull_intro", branch = local, remote = remote),
                t!("drop.pull_button"),
            ),
            DropPlan::Rewrite { older, newer } => (
                t!("drop.label_rewrite"),
                if squash {
                    t!(
                        "drop.squash_intro",
                        newer = subject(newer),
                        older = subject(older)
                    )
                } else {
                    t!(
                        "drop.swap_intro",
                        newer = subject(newer),
                        older = subject(older)
                    )
                },
                if squash {
                    t!("drop.squash_button")
                } else {
                    t!("drop.swap_button")
                },
            ),
            DropPlan::Integrate { .. } | DropPlan::NewWorkspace(_) | DropPlan::Refused(_) => {
                let label = self.plan_label(plan);
                (label.clone().into(), label.into(), t!("common.ok"))
            }
        };
        (title.into_owned(), intro.into_owned(), confirm.into_owned())
    }

    /// The label following the pointer while something is dragged.
    pub(super) fn drag_overlay(&self, p: &'static Palette) -> Option<Element<'_, Message>> {
        let drag = self.dragging()?;
        let (label, refused) = match self.current_plan() {
            Some(DropPlan::Refused(reason)) => (reason, true),
            Some(plan) => (self.plan_label(&plan), false),
            None => (
                match &drag.grip {
                    Grip::Commit(hash) => t!("drop.drag_commit", commit = short(hash)),
                    Grip::Branch(name) => t!("drop.drag_branch", branch = name),
                    Grip::Workspaces => return None,
                }
                .into_owned(),
                false,
            ),
        };
        let x = (self.pointer.x + 16.0)
            .min(self.window.width - 360.0)
            .max(0.0);
        let y = (self.pointer.y + 18.0)
            .min(self.window.height - 60.0)
            .max(0.0);
        Some(
            container(
                container(text(label).size(13).color(if refused {
                    p.warning.text
                } else {
                    p.text
                }))
                .padding([6, 10])
                .max_width(340)
                .style(style::popup),
            )
            .padding(Padding::default().left(x).top(y))
            .into(),
        )
    }
}
