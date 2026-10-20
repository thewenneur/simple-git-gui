//! "Fetch" and the "Send" wizard: the server's work is always fetched and
//! integrated before pushing, so a push is never refused for being behind.

use super::*;
use crate::git::ops::{self, Integration};
use crate::ui::style::StepState;
use iced::widget::{column, row};

pub struct SendWizard {
    pub branch: String,
    pub remote: String,
    /// The branch on the server, once known to exist.
    pub upstream: Option<String>,
    /// Whether the push must also record the upstream (`push -u`).
    pub set_upstream: bool,
    pub phase: SendPhase,
    pub to_send: Option<u32>,
    pub incoming: Vec<(String, String)>,
    pub how: Integration,
    /// The repository's `pull.rebase` setting, if set.
    pub configured: Option<Integration>,
    pub fetch_failed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendPhase {
    Fetching,
    Ready,
    Sending,
}

/// What the wizard learned from the server before sending.
#[derive(Debug, Clone)]
pub struct Prepared {
    records: Vec<CommandRecord>,
    fetched: bool,
    upstream: Option<String>,
    incoming: Vec<(String, String)>,
    to_send: Option<u32>,
    configured: Option<Integration>,
}

async fn configured_integration(
    root: &Path,
    records: &mut Vec<CommandRecord>,
) -> Option<Integration> {
    // Exits with 1 when unset, which is not an error: kept in the background.
    let record = run(ops::pull_rebase_setting(root).background()).await;
    let how = record
        .success()
        .then(|| Integration::from_config(Some(&record.stdout)));
    records.push(record);
    how
}

async fn prepare(
    root: PathBuf,
    remote: String,
    branch: String,
    upstream: Option<String>,
) -> Prepared {
    let fetch = run(ops::fetch(&root, &remote)).await;
    let fetched = fetch.success();
    let mut records = vec![fetch];
    let mut prepared = Prepared {
        records: Vec::new(),
        fetched,
        upstream: None,
        incoming: Vec::new(),
        to_send: None,
        configured: None,
    };
    if fetched {
        prepared.configured = configured_integration(&root, &mut records).await;
        // A branch never linked to the server may still exist there.
        let known = upstream.is_some();
        let candidate = upstream.unwrap_or_else(|| format!("{remote}/{branch}"));
        let mut command = ops::incoming(&root, &candidate);
        if !known {
            command = command.background();
        }
        let incoming = run(command).await;
        if incoming.success() {
            prepared.incoming = ops::parse_incoming(&incoming.stdout);
            prepared.upstream = Some(candidate);
        }
        records.push(incoming);
        let count = run(ops::outgoing_count(
            &root,
            prepared.upstream.as_deref(),
            &remote,
        ))
        .await;
        prepared.to_send = count.stdout.trim().parse().ok();
        records.push(count);
    }
    prepared.records = records;
    prepared
}

async fn fetch_and_integrate(
    root: PathBuf,
    remote: String,
    upstream: Option<String>,
) -> Vec<CommandRecord> {
    let fetch = run(ops::fetch(&root, &remote)).await;
    let ok = fetch.success();
    let mut records = vec![fetch];
    let Some(upstream) = upstream.filter(|_| ok) else {
        return records;
    };
    let how = configured_integration(&root, &mut records)
        .await
        .unwrap_or_default();
    let incoming = run(ops::incoming(&root, &upstream)).await;
    let behind = incoming.success() && !incoming.stdout.trim().is_empty();
    records.push(incoming);
    if behind {
        records.push(run(ops::integrate(&root, &upstream, how)).await);
    }
    records
}

impl App {
    /// The remote and upstream used by Fetch and Send, if the repository has
    /// a server.
    fn server(&self) -> Option<(String, Option<String>)> {
        let repo = self.repo.as_ref()?;
        let status = repo.status.as_ref()?;
        let upstream = status.branch.upstream.clone();
        let remote = upstream
            .as_deref()
            .and_then(|u| ops::remote_of(u, &repo.remotes))
            .or_else(|| ops::default_remote(&repo.remotes))?
            .to_string();
        Some((remote, upstream))
    }

    pub(super) fn can_fetch(&self) -> bool {
        self.idle()
            && self.server().is_some()
            && self.repo.as_ref().is_some_and(|r| r.operation.is_none())
    }

    pub(super) fn can_send(&self) -> bool {
        let Some(status) = self.repo.as_ref().and_then(|r| r.status.as_ref()) else {
            return false;
        };
        let b = &status.branch;
        self.idle()
            && self.server().is_some()
            && b.head.is_some()
            && b.oid.is_some()
            && (b.upstream.is_none() || b.ahead > 0)
            && self.repo.as_ref().is_some_and(|r| r.operation.is_none())
    }

    pub(super) fn fetch(&mut self) -> Task<Message> {
        let Some((remote, upstream)) = self.server() else {
            return Task::none();
        };
        let root = self.repo.as_ref().map(|r| r.paths.root.clone()).unwrap();
        self.busy = Some(t!("busy.fetching", remote = remote).into_owned());
        self.notice = None;
        Task::perform(fetch_and_integrate(root, remote, upstream), |records| {
            Message::ActionDone(Action::Fetch, records)
        })
    }

    pub(super) fn open_send(&mut self) -> Task<Message> {
        if !self.can_send() {
            return Task::none();
        }
        let Some((remote, upstream)) = self.server() else {
            return Task::none();
        };
        let repo = self.repo.as_ref().unwrap();
        let status = repo.status.as_ref().unwrap();
        let branch = status.branch.head.clone().unwrap_or_default();
        self.notice = None;
        self.send = Some(SendWizard {
            branch: branch.clone(),
            remote: remote.clone(),
            set_upstream: upstream.is_none(),
            upstream: upstream.clone(),
            phase: SendPhase::Fetching,
            to_send: None,
            incoming: Vec::new(),
            how: Integration::default(),
            configured: None,
            fetch_failed: false,
            error: None,
        });
        self.busy = Some(String::new());
        let root = repo.paths.root.clone();
        Task::perform(prepare(root, remote, branch, upstream), |prepared| {
            Message::SendPrepared(Box::new(prepared))
        })
    }

    pub(super) fn send_prepared(&mut self, prepared: Prepared) -> Task<Message> {
        self.busy = None;
        let failure = prepared
            .records
            .first()
            .filter(|r| !r.success())
            .map(|r| r.error_summary());
        for record in prepared.records {
            self.log(record);
        }
        if let Some(wizard) = &mut self.send {
            wizard.phase = SendPhase::Ready;
            wizard.fetch_failed = !prepared.fetched;
            wizard.error = failure.map(|e| t!("error.fetch_failed", detail = e).into_owned());
            wizard.upstream = prepared.upstream;
            wizard.incoming = prepared.incoming;
            wizard.to_send = prepared.to_send;
            wizard.configured = prepared.configured;
            wizard.how = prepared.configured.unwrap_or_default();
        }
        Task::none()
    }

    pub(super) fn confirm_send(&mut self) -> Task<Message> {
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let Some(wizard) = &mut self.send else {
            return Task::none();
        };
        if wizard.phase != SendPhase::Ready {
            return Task::none();
        }
        if wizard.fetch_failed {
            // "Try again": start over from the fetch.
            self.send = None;
            return self.open_send();
        }
        let root = &repo.paths.root;
        let mut commands = Vec::new();
        if let Some(upstream) = wizard
            .upstream
            .as_deref()
            .filter(|_| !wizard.incoming.is_empty())
        {
            commands.push(ops::integrate(root, upstream, wizard.how));
        }
        commands.push(ops::push(
            root,
            &wizard.remote,
            &wizard.branch,
            wizard.set_upstream,
        ));
        wizard.phase = SendPhase::Sending;
        wizard.error = None;
        self.busy = Some(String::new());
        Task::perform(run_all(commands), Message::SendDone)
    }

    pub(super) fn send_done(&mut self, records: Vec<CommandRecord>) -> Task<Message> {
        self.busy = None;
        let failed = records
            .last()
            .filter(|r| !r.success())
            .map(|r| (r.command.args.first().cloned(), r.error_summary()));
        for record in records {
            self.log(record);
        }
        match failed {
            None => {
                if let Some(w) = self.send.take() {
                    self.notice =
                        Some(t!("send.done", remote = w.remote, branch = w.branch).into_owned());
                }
            }
            // The integration stopped on a conflict: the wizard closes and
            // the banner offers to put everything back.
            Some((Some(verb), detail)) if verb == "rebase" || verb == "merge" => {
                self.send = None;
                self.error = Some(t!("error.integrate_conflict", detail = detail).into_owned());
            }
            Some((_, detail)) => {
                if let Some(w) = &mut self.send {
                    w.phase = SendPhase::Ready;
                    // Someone may have pushed meanwhile: fetch again on retry.
                    w.fetch_failed = true;
                    w.error = Some(t!("error.push_failed", detail = detail).into_owned());
                }
            }
        }
        self.refresh(false)
    }

    pub(super) fn send_dialog<'a>(
        &'a self,
        w: &'a SendWizard,
        p: &'static Palette,
    ) -> Element<'a, Message> {
        let root = &self.repo.as_ref().unwrap().paths.root;
        let fetching = w.phase == SendPhase::Fetching;

        let mut title = row![
            text(t!("send.title", branch = w.branch))
                .size(20)
                .font(sans(Weight::Semibold))
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if let Some(n) = w.to_send {
            let label = if n == 1 {
                t!("send.commits_one").into_owned()
            } else {
                t!("send.commits_many", count = n).into_owned()
            };
            title = title.push(
                container(text(label).size(12).font(sans(Weight::Semibold)))
                    .padding([2, 8])
                    .style(style::badge(p.accent.soft, p.accent.text)),
            );
        }

        // Step 1: fetch.
        let fetch_state = if fetching {
            StepState::Current
        } else if w.fetch_failed && w.to_send.is_none() {
            StepState::Failed
        } else {
            StepState::Done
        };
        let fetch_text = if fetching {
            t!("send.fetching").into_owned()
        } else if w.to_send.is_none() {
            t!("send.fetch_failed").into_owned()
        } else {
            incoming_summary(&w.incoming, &w.branch)
        };
        let step1 = step(
            1,
            fetch_state,
            t!("send.step_fetch").into_owned(),
            column![text(fetch_text).color(p.text_2)].into(),
            Some(ops::fetch(root, &w.remote).display()),
            p,
        );

        // Step 2: integrate.
        let integrating = !w.incoming.is_empty();
        let mut body = column![].spacing(8);
        let mut integrate_command = None;
        if fetching {
            body = body.push(text(t!("send.waiting")).color(p.muted));
        } else if !integrating {
            body = body.push(text(t!("send.nothing_to_integrate")).color(p.text_2));
        } else {
            let recommended = w.configured.unwrap_or_default();
            for (how, key, hint) in [
                (Integration::Rebase, "send.rebase", "send.rebase_hint"),
                (Integration::Merge, "send.merge", "send.merge_hint"),
            ] {
                let selected = w.how == how;
                let mut label = row![text(tr(key)).font(sans(Weight::Semibold))].spacing(4);
                if how == recommended {
                    label = label.push(text(t!("send.recommended")).color(p.accent.text));
                }
                let mut hint = tr(hint);
                if how == recommended {
                    let setting = match w.configured {
                        Some(Integration::Rebase) => t!("send.setting", value = "true"),
                        Some(Integration::Merge) => t!("send.setting", value = "false"),
                        None => t!("send.setting_unset"),
                    };
                    hint = format!("{hint} {setting}");
                }
                body = body.push(
                    button(
                        row![
                            radio_dot(selected, p),
                            column![label, text(hint).size(13).color(p.text_2)].spacing(2),
                        ]
                        .spacing(10),
                    )
                    .padding([10, 12])
                    .width(Length::Fill)
                    .on_press_maybe(
                        (w.phase == SendPhase::Ready).then_some(Message::SetIntegration(how)),
                    )
                    .style(style::choice(selected)),
                );
            }
            if let Some(upstream) = &w.upstream {
                integrate_command = Some(ops::integrate(root, upstream, w.how).display());
            }
        }
        let step2_state = match w.phase {
            SendPhase::Fetching => StepState::Todo,
            SendPhase::Ready if !integrating => StepState::Done,
            _ => StepState::Current,
        };
        let step2 = step(
            2,
            step2_state,
            t!("send.step_integrate").into_owned(),
            body.into(),
            integrate_command,
            p,
        );

        // Step 3: push.
        let step3 = step(
            3,
            if w.phase == SendPhase::Sending {
                StepState::Current
            } else {
                StepState::Todo
            },
            t!("send.step_push").into_owned(),
            text(t!("send.push_hint")).color(p.text_2).into(),
            Some(ops::push(root, &w.remote, &w.branch, w.set_upstream).display()),
            p,
        );

        let mut dialog = column![
            container(title).padding(Padding::from([20, 24]).bottom(4)),
            container(text(t!("send.intro")).color(p.text_2))
                .padding(Padding::from([4, 24]).bottom(12)),
            container(column![step1, step2, step3]).padding([0, 24]),
        ];
        if let Some(error) = &w.error {
            dialog = dialog.push(
                container(
                    container(text(error.clone()))
                        .padding([10, 12])
                        .width(Length::Fill)
                        .style(style::banner_error),
                )
                .padding(Padding::from([4, 24]).top(8)),
            );
        }

        let confirm = if w.fetch_failed {
            t!("send.retry")
        } else if integrating {
            t!("send.integrate_and_send")
        } else {
            t!("send.send")
        };
        dialog = dialog.push(
            container(
                row![
                    space::horizontal(),
                    button(centered(text(t!("common.cancel"))))
                        .padding([0, 16])
                        .height(40)
                        .on_press_maybe(
                            (w.phase != SendPhase::Sending).then_some(Message::CloseSend),
                        )
                        .style(style::secondary),
                    button(centered(text(confirm).font(sans(Weight::Semibold))))
                        .padding([0, 18])
                        .height(40)
                        .on_press_maybe(
                            (w.phase == SendPhase::Ready).then_some(Message::ConfirmSend)
                        )
                        .style(style::primary),
                ]
                .spacing(8),
            )
            .padding([20, 24]),
        );

        container(dialog).max_width(640).style(style::dialog).into()
    }
}

/// "Léa sent 1 commit on feature/x: « … »."
fn incoming_summary(incoming: &[(String, String)], branch: &str) -> String {
    if incoming.is_empty() {
        return t!("send.no_incoming").into_owned();
    }
    let mut authors: Vec<&str> = Vec::new();
    for (author, _) in incoming {
        if !authors.contains(&author.as_str()) {
            authors.push(author);
        }
    }
    let mut subjects: Vec<String> = incoming
        .iter()
        .take(3)
        .map(|(_, s)| t!("send.quoted", subject = s).into_owned())
        .collect();
    if incoming.len() > 3 {
        subjects.push("…".to_string());
    }
    let key = if incoming.len() == 1 {
        "send.incoming_one"
    } else {
        "send.incoming_many"
    };
    t!(
        key,
        authors = authors.join(", "),
        count = incoming.len(),
        branch = branch,
        subjects = subjects.join(", ")
    )
    .into_owned()
}

fn radio_dot<'a>(selected: bool, p: &'static Palette) -> Element<'a, Message> {
    let inner: Element<'a, Message> = if selected {
        container(Space::new())
            .width(8)
            .height(8)
            .style(style::dot(p.accent.strong))
            .into()
    } else {
        Space::new().into()
    };
    container(container(inner).center(Length::Fill))
        .width(16)
        .height(16)
        .style(style::radio(selected))
        .into()
}

fn step<'a>(
    number: u8,
    state: StepState,
    title: String,
    body: Element<'a, Message>,
    command: Option<String>,
    p: &'static Palette,
) -> Element<'a, Message> {
    let mark = match state {
        StepState::Done => icon(Icon::Check, 14.0, Color::WHITE),
        StepState::Failed => icon(Icon::Cross, 14.0, Color::WHITE),
        _ => text(number.to_string()).font(sans(Weight::Semibold)).into(),
    };
    let mut content = column![text(title).font(sans(Weight::Semibold)), body].spacing(6);
    if let Some(command) = command {
        content = content.push(
            text(format!("$ {command}"))
                .size(12)
                .font(style::MONO)
                .color(p.text_2),
        );
    }
    column![
        hline(style::separator),
        container(
            row![
                container(container(mark).center(Length::Fill))
                    .width(28)
                    .height(28)
                    .style(style::step_marker(state)),
                content.width(Length::Fill),
            ]
            .spacing(12),
        )
        .padding([12, 0]),
    ]
    .into()
}
