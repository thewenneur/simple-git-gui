//! Servers (remotes) of the open repository, in the Git configuration
//! screen, and the "new repository" card of the welcome screen.

use iced::widget::{column, row};

use super::undo::Undoable;
use super::*;
use crate::git::remotes::{self, Remote};
use crate::git::templates;
use crate::git::undo::{Inverse, Moves};

/// What is being edited in the servers card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editing {
    Add,
    Rename(String),
    Url(String),
}

#[derive(Debug, Default)]
pub struct RemotesState {
    pub list: Option<Vec<Remote>>,
    pub editing: Option<Editing>,
    pub name: String,
    pub url: String,
    /// A separate address for sending (empty: the same as `url`).
    pub push_url: String,
}

/// The "new repository" form of the welcome screen.
#[derive(Debug)]
pub struct NewRepo {
    pub name: String,
    pub parent: Option<PathBuf>,
    pub branch: String,
    pub templates: Vec<&'static str>,
    pub readme: bool,
    pub remote: String,
}

impl Default for NewRepo {
    fn default() -> Self {
        Self {
            name: String::new(),
            parent: None,
            branch: "main".to_string(),
            templates: Vec::new(),
            readme: true,
            remote: String::new(),
        }
    }
}

impl NewRepo {
    fn folder(&self) -> Option<PathBuf> {
        let name = self.name.trim();
        (!name.is_empty() && !name.contains(['/', '\\']))
            .then(|| self.parent.as_ref().map(|p| p.join(name)))
            .flatten()
    }

    fn ready(&self) -> bool {
        self.folder().is_some()
            && remotes::valid_name(self.branch.trim())
            && (self.remote.trim().is_empty() || !self.remote.trim().contains(char::is_whitespace))
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Loaded(PathBuf, Box<CommandRecord>),
    Edit(Option<Editing>),
    Name(String),
    Url(String),
    PushUrl(String),
    Apply,
    Remove(String),
    Test(String),
    // The new repository form.
    NewName(String),
    PickNewParent,
    NewParentPicked(Option<PathBuf>),
    NewBranch(String),
    ToggleTemplate(&'static str),
    ToggleReadme(bool),
    NewRemote(String),
    Create,
    Created(PathBuf, Vec<CommandRecord>),
}

/// Creates the repository: folder, `git init`, first files, first commit and
/// server address. Stops at the first failure.
async fn create(
    dir: PathBuf,
    branch: String,
    templates_ids: Vec<&'static str>,
    readme: bool,
    remote: String,
) -> Vec<CommandRecord> {
    let mut records = Vec::new();
    let parent = dir.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = folder_name(&dir);
    let init = run(remotes::init(&dir, &branch).in_dir(&parent)).await;
    let ok = init.success();
    records.push(init);
    if !ok {
        return records;
    }
    let mut write = |file: &str, content: String, shell: String| {
        let result = std::fs::write(dir.join(file), content).map_err(|e| e.to_string());
        let ok = result.is_ok();
        records.push(CommandRecord::local(shell, &dir, result));
        ok
    };
    let mut ignore = String::new();
    for id in &templates_ids {
        if let Some(template) = templates::find(id) {
            ignore = templates::append(&ignore, template).unwrap_or(ignore);
        }
    }
    let mut files = Vec::new();
    if !ignore.is_empty() {
        let shell = format!("cat > .gitignore <<'EOF'\n{ignore}EOF");
        if !write(".gitignore", ignore, shell) {
            return records;
        }
        files.push(".gitignore");
    }
    if readme {
        let content = format!("# {name}\n");
        let shell = format!("echo {} > README.md", runner_quote(content.trim_end()));
        if !write("README.md", content, shell) {
            return records;
        }
        files.push("README.md");
    }
    if !files.is_empty() {
        let mut add = vec!["add", "--"];
        add.extend(&files);
        let staged = run(GitCommand::new(add).in_dir(&dir)).await;
        let ok = staged.success();
        records.push(staged);
        if ok {
            records.push(run(ops::commit(&dir, "Initial commit")).await);
        }
    }
    if !remote.is_empty() {
        records.push(run(remotes::add(&dir, "origin", &remote)).await);
    }
    records
}

fn runner_quote(text: &str) -> String {
    crate::git::runner::quote(text)
}

impl App {
    pub(super) fn load_remotes(&mut self) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let root = repo.paths.root.clone();
        Task::perform(run(remotes::list_command(&root)), move |record| {
            Message::Remotes(Msg::Loaded(root.clone(), Box::new(record)))
        })
    }

    pub(super) fn remotes_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Loaded(root, record) => {
                if let Some(repo) = self.repo.as_mut().filter(|r| r.paths.root == root)
                    && record.success()
                {
                    repo.remotes_state.list = Some(remotes::parse(&record.stdout));
                }
                self.log(*record);
                Task::none()
            }
            Msg::Edit(editing) => {
                if let Some(repo) = &mut self.repo {
                    let state = &mut repo.remotes_state;
                    let current = match &editing {
                        Some(Editing::Rename(n) | Editing::Url(n)) => state
                            .list
                            .as_ref()
                            .and_then(|l| l.iter().find(|r| &r.name == n))
                            .cloned(),
                        _ => None,
                    };
                    state.name = match (&editing, &current) {
                        (Some(Editing::Add), _) => {
                            let taken = |n: &str| {
                                state
                                    .list
                                    .as_ref()
                                    .is_some_and(|l| l.iter().any(|r| r.name == n))
                            };
                            if taken("origin") {
                                String::new()
                            } else {
                                "origin".into()
                            }
                        }
                        (_, Some(r)) => r.name.clone(),
                        _ => String::new(),
                    };
                    state.url = current
                        .as_ref()
                        .map(|r| r.fetch.clone())
                        .unwrap_or_default();
                    state.push_url = current.and_then(|r| r.push).unwrap_or_default();
                    state.editing = editing;
                }
                Task::none()
            }
            Msg::Name(v) => self.edit_remote_form(|s| s.name = v),
            Msg::Url(v) => self.edit_remote_form(|s| s.url = v),
            Msg::PushUrl(v) => self.edit_remote_form(|s| s.push_url = v),
            Msg::Apply => self.apply_remote_edit(),
            Msg::Remove(name) => {
                let Some(repo) = &self.repo else {
                    return Task::none();
                };
                let root = repo.paths.root.clone();
                let Some(remote) = repo
                    .remotes_state
                    .list
                    .as_ref()
                    .and_then(|l| l.iter().find(|r| r.name == name))
                    .cloned()
                else {
                    return Task::none();
                };
                // Undoing adds it back; its branches come back with the next Get.
                let mut back = vec![remotes::add(&root, &name, &remote.fetch)];
                if let Some(push) = &remote.push {
                    back.push(remotes::set_url(&root, &name, push, true));
                }
                let mut undoable = Undoable::new(
                    t!("undo.remote_remove", name = name).into_owned(),
                    Moves::Index,
                );
                undoable.inverse = Some(Inverse {
                    undo: back,
                    redo: vec![remotes::remove(&root, &name)],
                });
                self.perform(
                    Action::Remote,
                    None,
                    Some(undoable),
                    vec![remotes::remove(&root, &name)],
                )
            }
            Msg::Test(name) => {
                let Some(root) = self.repo.as_ref().map(|r| r.paths.root.clone()) else {
                    return Task::none();
                };
                self.busy = Some(t!("busy.testing_remote", name = name).into_owned());
                Task::perform(run(remotes::test(&root, &name)), move |record| {
                    Message::ActionDone(Action::RemoteTest, vec![record], None)
                })
            }
            Msg::NewName(v) => {
                self.new_repo.name = v;
                Task::none()
            }
            Msg::PickNewParent => {
                Task::perform(pick_folder(t!("dialog.new_parent").into_owned()), |dir| {
                    Message::Remotes(Msg::NewParentPicked(dir))
                })
            }
            Msg::NewParentPicked(dir) => {
                if dir.is_some() {
                    self.new_repo.parent = dir;
                }
                Task::none()
            }
            Msg::NewBranch(v) => {
                self.new_repo.branch = v;
                Task::none()
            }
            Msg::ToggleTemplate(id) => {
                let list = &mut self.new_repo.templates;
                match list.iter().position(|t| *t == id) {
                    Some(i) => {
                        list.remove(i);
                    }
                    None => list.push(id),
                }
                Task::none()
            }
            Msg::ToggleReadme(on) => {
                self.new_repo.readme = on;
                Task::none()
            }
            Msg::NewRemote(v) => {
                self.new_repo.remote = v;
                Task::none()
            }
            Msg::Create => {
                let form = &self.new_repo;
                let Some(dir) = form.folder().filter(|_| form.ready()) else {
                    return Task::none();
                };
                if dir.join(".git").exists() {
                    self.error = Some(t!("error.already_repo", path = dir.display()).into_owned());
                    return Task::none();
                }
                self.busy = Some(t!("busy.creating", name = folder_name(&dir)).into_owned());
                self.error = None;
                let work = create(
                    dir.clone(),
                    form.branch.trim().to_string(),
                    form.templates.clone(),
                    form.readme,
                    form.remote.trim().to_string(),
                );
                Task::perform(work, move |records| {
                    Message::Remotes(Msg::Created(dir.clone(), records))
                })
            }
            Msg::Created(dir, records) => {
                self.busy = None;
                let initialised = records.first().is_some_and(CommandRecord::success);
                if let Some(failed) = records.iter().find(|r| !r.success()) {
                    self.error = Some(if ops::is_missing_identity(&failed.stderr) {
                        t!("error.identity").into_owned()
                    } else {
                        t!("error.create_failed", detail = failed.error_summary()).into_owned()
                    });
                }
                for record in records {
                    self.log(record);
                }
                if initialised {
                    self.new_repo = NewRepo {
                        parent: self.new_repo.parent.take(),
                        ..NewRepo::default()
                    };
                    self.open(dir)
                } else {
                    Task::none()
                }
            }
        }
    }

    fn edit_remote_form(&mut self, change: impl FnOnce(&mut RemotesState)) -> Task<Message> {
        if let Some(repo) = &mut self.repo {
            change(&mut repo.remotes_state);
        }
        Task::none()
    }

    /// The commands of the edit being typed, and how to undo them.
    fn remote_edit_commands(
        &self,
        repo: &OpenRepo,
    ) -> Option<(String, Vec<GitCommand>, Vec<GitCommand>)> {
        let state = &repo.remotes_state;
        let root = &repo.paths.root;
        let name = state.name.trim();
        let url = state.url.trim();
        let push = state.push_url.trim();
        let list = state.list.as_deref().unwrap_or_default();
        let taken = |n: &str| list.iter().any(|r| r.name == n);
        match state.editing.as_ref()? {
            Editing::Add => {
                if !remotes::valid_name(name) || taken(name) || url.is_empty() {
                    return None;
                }
                let mut commands = vec![remotes::add(root, name, url)];
                if !push.is_empty() && push != url {
                    commands.push(remotes::set_url(root, name, push, true));
                }
                Some((
                    t!("undo.remote_add", name = name).into_owned(),
                    commands,
                    vec![remotes::remove(root, name)],
                ))
            }
            Editing::Rename(old) => {
                if !remotes::valid_name(name) || name == old || taken(name) {
                    return None;
                }
                Some((
                    t!("undo.remote_rename", from = old, to = name).into_owned(),
                    vec![remotes::rename(root, old, name)],
                    vec![remotes::rename(root, name, old)],
                ))
            }
            Editing::Url(remote) => {
                let current = list.iter().find(|r| &r.name == remote)?;
                if url.is_empty() {
                    return None;
                }
                let mut commands = Vec::new();
                let mut back = Vec::new();
                if url != current.fetch {
                    commands.push(remotes::set_url(root, remote, url, false));
                    back.push(remotes::set_url(root, remote, &current.fetch, false));
                }
                let old_push = current.push.as_deref().unwrap_or("");
                let new_push = if push == url { "" } else { push };
                if new_push != old_push {
                    commands.push(if new_push.is_empty() {
                        remotes::unset_push_url(root, remote)
                    } else {
                        remotes::set_url(root, remote, new_push, true)
                    });
                    back.push(if old_push.is_empty() {
                        remotes::unset_push_url(root, remote)
                    } else {
                        remotes::set_url(root, remote, old_push, true)
                    });
                }
                if commands.is_empty() {
                    return None;
                }
                Some((
                    t!("undo.remote_url", name = remote).into_owned(),
                    commands,
                    back,
                ))
            }
        }
    }

    fn apply_remote_edit(&mut self) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let Some((label, commands, back)) = self.remote_edit_commands(repo) else {
            return Task::none();
        };
        let mut undoable = Undoable::new(label, Moves::Index);
        undoable.inverse = Some(Inverse {
            undo: back,
            redo: commands.clone(),
        });
        if let Some(repo) = &mut self.repo {
            repo.remotes_state.editing = None;
        }
        self.perform(Action::Remote, None, Some(undoable), commands)
    }

    /// The servers card of the Git configuration screen.
    pub(super) fn remotes_card<'a>(
        &'a self,
        repo: &'a OpenRepo,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let state = &repo.remotes_state;
        let enabled = self.idle();
        let mut card = column![
            container(
                row![
                    column![
                        text(t!("remotes.title"))
                            .size(15)
                            .font(sans(Weight::Semibold)),
                        text(t!("remotes.intro")).size(13).color(p.text_2),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    small_button(
                        t!("remotes.add").into_owned(),
                        (enabled && state.editing != Some(Editing::Add))
                            .then_some(Message::Remotes(Msg::Edit(Some(Editing::Add)))),
                        true,
                    ),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([12, 16]),
        ];
        if state.editing == Some(Editing::Add) {
            card = card
                .push(hline(style::separator))
                .push(self.remote_form(repo, p));
        }
        let list = state.list.as_deref().unwrap_or_default();
        if list.is_empty() && state.editing.is_none() {
            card = card.push(hline(style::separator)).push(
                container(text(t!("remotes.none")).size(13).color(p.muted)).padding([14, 16]),
            );
        }
        for remote in list {
            card = card.push(hline(style::separator));
            let name = remote.name.clone();
            let mut urls = column![
                text(remote.fetch.clone())
                    .font(style::MONO)
                    .size(12)
                    .color(p.text_2)
            ]
            .spacing(2);
            if let Some(push) = &remote.push {
                urls = urls.push(
                    text(t!("remotes.push_to", url = push))
                        .size(12)
                        .color(p.muted),
                );
            }
            let line = row![
                column![
                    text(remote.name.clone())
                        .font(mono(Weight::Semibold))
                        .size(13),
                    urls,
                ]
                .spacing(2)
                .width(Length::Fill),
                small_button(
                    t!("remotes.test").into_owned(),
                    enabled.then(|| Message::Remotes(Msg::Test(name.clone()))),
                    false,
                ),
                small_button(
                    t!("remotes.change_url").into_owned(),
                    enabled.then(|| Message::Remotes(Msg::Edit(Some(Editing::Url(name.clone()))))),
                    false,
                ),
                small_button(
                    t!("remotes.rename").into_owned(),
                    enabled
                        .then(|| Message::Remotes(Msg::Edit(Some(Editing::Rename(name.clone()))))),
                    false,
                ),
                button(centered(text(t!("remotes.remove")).size(13)))
                    .padding([0, 10])
                    .height(30)
                    .on_press_maybe(enabled.then(|| Message::Remotes(Msg::Remove(name.clone()))))
                    .style(style::danger),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            card = card.push(container(line).padding([12, 16]));
            let editing_this = matches!(
                &state.editing,
                Some(Editing::Rename(n) | Editing::Url(n)) if n == &remote.name
            );
            if editing_this {
                card = card.push(self.remote_form(repo, p));
            }
        }
        container(card)
            .width(Length::Fill)
            .style(style::card)
            .into()
    }

    fn remote_form<'a>(&'a self, repo: &'a OpenRepo, p: &'static Palette) -> Element<'a, Message> {
        let state = &repo.remotes_state;
        let Some(editing) = &state.editing else {
            return Space::new().into();
        };
        let field = |label: String, placeholder: String, value: &'a str, on: fn(String) -> Msg| {
            column![
                text(label).size(12).color(p.muted),
                text_input(&placeholder, value)
                    .on_input(move |v| Message::Remotes(on(v)))
                    .on_submit(Message::Remotes(Msg::Apply))
                    .font(style::MONO)
                    .size(13)
                    .padding([8, 10])
                    .style(style::input),
            ]
            .spacing(4)
        };
        let (intro, fields): (String, Element<'a, Message>) = match editing {
            Editing::Add => (
                t!("remotes.add_intro").into_owned(),
                row![
                    field(
                        t!("remotes.name").into_owned(),
                        "origin".into(),
                        &state.name,
                        Msg::Name
                    )
                    .width(Length::FillPortion(1)),
                    field(
                        t!("remotes.url").into_owned(),
                        t!("remotes.url_placeholder").into_owned(),
                        &state.url,
                        Msg::Url
                    )
                    .width(Length::FillPortion(3)),
                ]
                .spacing(12)
                .into(),
            ),
            Editing::Rename(old) => (
                t!("remotes.rename_intro", name = old).into_owned(),
                field(
                    t!("remotes.new_name").into_owned(),
                    old.clone(),
                    &state.name,
                    Msg::Name,
                )
                .width(240)
                .into(),
            ),
            Editing::Url(_) => (
                t!("remotes.url_intro").into_owned(),
                row![
                    field(
                        t!("remotes.url").into_owned(),
                        t!("remotes.url_placeholder").into_owned(),
                        &state.url,
                        Msg::Url
                    )
                    .width(Length::FillPortion(1)),
                    field(
                        t!("remotes.push_url").into_owned(),
                        t!("remotes.push_url_placeholder").into_owned(),
                        &state.push_url,
                        Msg::PushUrl
                    )
                    .width(Length::FillPortion(1)),
                ]
                .spacing(12)
                .into(),
            ),
        };
        let commands = self.remote_edit_commands(repo);
        let preview = match &commands {
            Some((_, commands, _)) => commands
                .iter()
                .map(|c| format!("$ {}", c.display()))
                .collect::<Vec<_>>()
                .join("\n"),
            None if matches!(editing, Editing::Rename(_)) => {
                t!("remotes.incomplete_name").into_owned()
            }
            None => t!("remotes.incomplete").into_owned(),
        };
        container(
            column![
                text(intro).size(13).color(p.text_2),
                fields,
                row![
                    text(preview)
                        .font(style::MONO)
                        .size(12)
                        .color(p.text_2)
                        .width(Length::Fill),
                    small_button(
                        t!("common.cancel").into_owned(),
                        Some(Message::Remotes(Msg::Edit(None))),
                        false
                    ),
                    small_button(
                        t!("remotes.apply").into_owned(),
                        (self.idle() && commands.is_some()).then_some(Message::Remotes(Msg::Apply)),
                        true
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(10),
        )
        .padding(Padding::default().left(16).right(16).top(4).bottom(14))
        .into()
    }

    /// The "new repository" card of the welcome screen.
    pub(super) fn new_repo_card<'a>(&'a self, p: &'static Palette) -> Element<'a, Message> {
        let form = &self.new_repo;
        let parent = match &form.parent {
            Some(path) => path.display().to_string(),
            None => t!("welcome.no_folder").into_owned(),
        };
        let mut chips = row![].spacing(6);
        for template in templates::TEMPLATES {
            let on = form.templates.contains(&template.id);
            chips = chips.push(
                button(centered(
                    text(if on {
                        format!("✓ {}", template.name)
                    } else {
                        template.name.to_string()
                    })
                    .size(12),
                ))
                .padding([0, 10])
                .height(28)
                .on_press(Message::Remotes(Msg::ToggleTemplate(template.id)))
                .style(style::choice(on)),
            );
        }
        let mut preview = Vec::new();
        if let Some(dir) = form.folder() {
            preview.push(remotes::init(&dir, form.branch.trim()).display());
            if !form.templates.is_empty() {
                preview.push(format!(
                    "# .gitignore: {}",
                    form.templates
                        .iter()
                        .filter_map(|id| templates::find(id))
                        .map(|t| t.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if form.readme || !form.templates.is_empty() {
                preview.push("git add -- …".to_string());
                preview.push(ops::commit(&dir, "Initial commit").display());
            }
            if !form.remote.trim().is_empty() {
                preview.push(remotes::add(&dir, "origin", form.remote.trim()).display());
            }
        }
        let ready = self.idle() && form.ready();
        column![
            text(t!("welcome.new_title"))
                .size(15)
                .font(sans(Weight::Semibold)),
            text(t!("welcome.new_hint")).color(p.text_2),
            row![
                column![
                    text(t!("welcome.new_name")).size(12).color(p.muted),
                    text_input("carte-fpga-v3", &form.name)
                        .on_input(|v| Message::Remotes(Msg::NewName(v)))
                        .size(13)
                        .padding([8, 10])
                        .style(style::input),
                ]
                .spacing(4)
                .width(Length::FillPortion(3)),
                column![
                    text(t!("welcome.new_branch")).size(12).color(p.muted),
                    text_input("main", &form.branch)
                        .on_input(|v| Message::Remotes(Msg::NewBranch(v)))
                        .font(style::MONO)
                        .size(13)
                        .padding([8, 10])
                        .style(style::input),
                ]
                .spacing(4)
                .width(Length::FillPortion(1)),
            ]
            .spacing(12),
            row![
                button(text(t!("welcome.new_pick_parent")))
                    .padding([8, 12])
                    .on_press_maybe(self.idle().then_some(Message::Remotes(Msg::PickNewParent)))
                    .style(style::secondary),
                text(parent).size(13).color(p.muted),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            text(t!("welcome.new_templates")).size(12).color(p.muted),
            chips.wrap(),
            checkbox(form.readme)
                .label(t!("welcome.new_readme"))
                .size(16)
                .on_toggle(|on| Message::Remotes(Msg::ToggleReadme(on)))
                .style(style::check),
            column![
                text(t!("welcome.new_remote")).size(12).color(p.muted),
                text_input(&t!("remotes.url_placeholder"), &form.remote)
                    .on_input(|v| Message::Remotes(Msg::NewRemote(v)))
                    .font(style::MONO)
                    .size(13)
                    .padding([8, 10])
                    .style(style::input),
            ]
            .spacing(4),
            (!preview.is_empty()).then(|| {
                text(
                    preview
                        .iter()
                        .map(|l| format!("$ {l}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .font(style::MONO)
                .size(12)
                .color(p.muted)
            }),
            button(text(t!("welcome.new_button")).font(sans(Weight::Semibold)))
                .padding([10, 16])
                .on_press_maybe(ready.then_some(Message::Remotes(Msg::Create)))
                .style(style::primary),
        ]
        .spacing(10)
        .into()
    }
}

fn small_button<'a>(
    label: String,
    message: Option<Message>,
    primary: bool,
) -> Element<'a, Message> {
    super::modules::small_button(label, message, primary)
}
