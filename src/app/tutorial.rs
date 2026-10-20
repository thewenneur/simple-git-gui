//! The interactive tutorial: short tracks of lessons played on a practice
//! repository. Each lesson points at the real interface (the rest is dimmed)
//! and waits for the user to do the action; it can be skipped, left and
//! resumed.

use iced::widget::{column, row};

use super::*;
use crate::git::practice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Track {
    Hardware,
    Design,
    Advanced,
}

impl Track {
    pub const ALL: [Track; 3] = [Track::Hardware, Track::Design, Track::Advanced];

    fn id(self) -> &'static str {
        match self {
            Track::Hardware => "hardware",
            Track::Design => "design",
            Track::Advanced => "advanced",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn lessons(self) -> &'static [Lesson] {
        use Lesson::*;
        match self {
            Track::Hardware => &[
                LookChange, Stage, Commit, Undo, Get, Send, Branch, Conflict, Journal,
            ],
            Track::Design => &[LookChange, Stage, Commit, Undo, Get, Send, Locks, Journal],
            Track::Advanced => &[Tour, Branch, Drag, Conflict, Workspace, Journal],
        }
    }
}

/// One lesson of a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lesson {
    LookChange,
    Stage,
    Commit,
    Undo,
    Get,
    Send,
    /// A look at the branch tree.
    Tour,
    Branch,
    Drag,
    Conflict,
    Locks,
    Workspace,
    Journal,
}

/// The part of the interface a lesson points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    FileList,
    Undo,
    Get,
    Send,
    History,
    Journal,
    Nav(Section),
}

/// The areas of the window, dimmed when a lesson doesn't use them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    TopBar,
    Sidebar,
    Content,
    Journal,
}

impl Spot {
    /// Whether the lesson works in this area of the window. A sidebar item
    /// opens a page, and that page is where the user acts.
    fn lights(self, region: Region) -> bool {
        match self {
            Spot::FileList | Spot::History => region == Region::Content,
            Spot::Undo | Spot::Get | Spot::Send => region == Region::TopBar,
            Spot::Journal => region == Region::Journal,
            Spot::Nav(_) => matches!(region, Region::Sidebar | Region::Content),
        }
    }
}

impl Lesson {
    fn key(self) -> &'static str {
        match self {
            Lesson::LookChange => "look_change",
            Lesson::Stage => "stage",
            Lesson::Commit => "commit",
            Lesson::Undo => "undo",
            Lesson::Get => "get",
            Lesson::Send => "send",
            Lesson::Tour => "tour",
            Lesson::Branch => "branch",
            Lesson::Drag => "drag",
            Lesson::Conflict => "conflict",
            Lesson::Locks => "locks",
            Lesson::Workspace => "workspace",
            Lesson::Journal => "journal",
        }
    }

    fn spot(self) -> Spot {
        match self {
            Lesson::LookChange | Lesson::Stage | Lesson::Commit => Spot::FileList,
            Lesson::Undo => Spot::Undo,
            Lesson::Get => Spot::Get,
            Lesson::Send => Spot::Send,
            Lesson::Tour | Lesson::Branch | Lesson::Drag | Lesson::Conflict => Spot::History,
            Lesson::Locks => Spot::Nav(Section::Locks),
            Lesson::Workspace => Spot::Nav(Section::Workspaces),
            Lesson::Journal => Spot::Journal,
        }
    }

    /// The screen shown when the lesson starts.
    fn section(self) -> Option<Section> {
        match self {
            Lesson::LookChange | Lesson::Stage | Lesson::Commit => Some(Section::Changes),
            Lesson::Tour | Lesson::Branch | Lesson::Drag | Lesson::Conflict => {
                Some(Section::History)
            }
            Lesson::Locks => Some(Section::Locks),
            Lesson::Workspace => Some(Section::Workspaces),
            _ => None,
        }
    }

    /// Lessons that only show something: they go on with "Next".
    fn info(self) -> bool {
        matches!(
            self,
            Lesson::LookChange | Lesson::Tour | Lesson::Locks | Lesson::Journal
        )
    }
}

/// The state when a lesson started, to tell when its action was done.
#[derive(Debug, Clone)]
struct Baseline {
    head: Option<String>,
    branches: usize,
    redo: usize,
    drops: u32,
}

pub struct Tutorial {
    pub track: Track,
    pub step: usize,
    /// The practice repository, as git reports it once open.
    pub repo: PathBuf,
    base: Option<Baseline>,
    /// The undo lesson: the action was undone, now waiting for the redo.
    undid: bool,
    /// The conflict lesson: the merge stopped on a conflict.
    saw_conflict: bool,
}

impl Tutorial {
    fn new(track: Track, step: usize, repo: PathBuf) -> Self {
        Self {
            track,
            step,
            repo,
            base: None,
            undid: false,
            saw_conflict: false,
        }
    }

    pub fn lesson(&self) -> Option<Lesson> {
        self.track.lessons().get(self.step).copied()
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// Shows the track choice.
    Offer,
    CloseOffer,
    Start(Track),
    Created(Track, usize, Result<PathBuf, String>, Vec<CommandRecord>),
    Skip,
    Next,
    Leave,
    Resume,
    Finish,
    BackToPractice,
    ToggleHelp,
}

const SEEN: &str = "tutorial.seen";
const TRACK: &str = "tutorial.track";
const STEP: &str = "tutorial.step";
const REPO: &str = "tutorial.repo";

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

impl App {
    /// Whether the tutorial was never offered (first launch).
    pub(super) fn tutorial_first_launch(&mut self) -> bool {
        let first = self.settings.get(SEEN).is_none();
        if first {
            self.settings.set(SEEN, Some("yes".into()));
        }
        first
    }

    /// Progress saved when the tutorial was left: track, step and folder.
    fn saved_progress(&self) -> Option<(Track, usize, PathBuf)> {
        let track = Track::from_id(self.settings.get(TRACK)?)?;
        let step = self.settings.get(STEP)?.parse().ok()?;
        let repo = PathBuf::from(self.settings.get(REPO)?);
        Some((track, step, repo))
    }

    pub(super) fn can_resume_tutorial(&self) -> bool {
        self.tutorial.is_none() && self.saved_progress().is_some()
    }

    fn save_progress(&mut self) {
        let Some(t) = &self.tutorial else {
            return;
        };
        let (track, step, repo) = (
            t.track.id().to_string(),
            t.step.to_string(),
            t.repo.to_string_lossy().into_owned(),
        );
        self.settings.set(TRACK, Some(track));
        self.settings.set(STEP, Some(step));
        self.settings.set(REPO, Some(repo));
    }

    fn forget_progress(&mut self) {
        for key in [TRACK, STEP, REPO] {
            self.settings.set(key, None);
        }
    }

    fn create_practice(&mut self, track: Track, step: usize) -> Task<Message> {
        self.busy = Some(t!("busy.practice").into_owned());
        let texts = practice::Texts {
            first: t!("practice.first").into_owned(),
            uart: t!("practice.uart").into_owned(),
            baud_fast: t!("practice.baud_fast").into_owned(),
            debounce: t!("practice.debounce").into_owned(),
            readme: t!("practice.readme").into_owned(),
            trial: t!("practice.trial").into_owned(),
            fallback_name: t!("practice.me").into_owned(),
        };
        Task::perform(
            async move { practice::create(&practice::base_dir(), &texts).await },
            move |(result, records)| Message::Tutorial(Msg::Created(track, step, result, records)),
        )
    }

    /// Called when a repository is open: recognises the practice one.
    pub(super) fn tutorial_opened(&mut self, root: &Path) {
        if let Some(t) = &mut self.tutorial
            && t.repo != root
            && same_dir(&t.repo, root)
        {
            t.repo = root.to_path_buf();
        }
    }

    pub(super) fn tutorial_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Offer => {
                self.help_open = false;
                self.tutorial_offer = true;
                Task::none()
            }
            Msg::CloseOffer => {
                self.tutorial_offer = false;
                Task::none()
            }
            Msg::Start(track) => {
                self.tutorial_offer = false;
                self.help_open = false;
                self.tutorial = None;
                self.create_practice(track, 0)
            }
            Msg::Created(track, step, result, records) => {
                self.busy = None;
                for record in records {
                    self.log(record);
                }
                match result {
                    Ok(repo) => {
                        self.tutorial = Some(Tutorial::new(track, step, repo.clone()));
                        self.save_progress();
                        self.open(repo)
                    }
                    Err(detail) => {
                        self.error = Some(t!("tutorial.failed", detail = detail).into_owned());
                        Task::none()
                    }
                }
            }
            Msg::Skip | Msg::Next => {
                self.advance();
                Task::none()
            }
            Msg::Leave => {
                self.help_open = false;
                self.save_progress();
                self.tutorial = None;
                self.notice = Some(t!("tutorial.left").into_owned());
                Task::none()
            }
            Msg::Resume => {
                self.help_open = false;
                let Some((track, step, repo)) = self.saved_progress() else {
                    return Task::none();
                };
                if repo.join(".git").exists() {
                    self.tutorial = Some(Tutorial::new(track, step, repo.clone()));
                    self.open(repo)
                } else {
                    // The temporary folder was cleaned: start a new one.
                    self.create_practice(track, step)
                }
            }
            Msg::Finish => {
                self.tutorial = None;
                self.forget_progress();
                Task::none()
            }
            Msg::BackToPractice => match &self.tutorial {
                Some(t) => {
                    let repo = t.repo.clone();
                    self.open(repo)
                }
                None => Task::none(),
            },
            Msg::ToggleHelp => {
                self.help_open = !self.help_open;
                Task::none()
            }
        }
    }

    fn advance(&mut self) {
        if let Some(t) = &mut self.tutorial {
            t.step += 1;
            t.base = None;
            t.undid = false;
            t.saw_conflict = false;
        }
        self.save_progress();
    }

    /// The lesson in progress, when the practice repository is open.
    fn active_lesson(&self) -> Option<Lesson> {
        let t = self.tutorial.as_ref()?;
        let repo = self.repo.as_ref()?;
        (repo.paths.root == t.repo).then(|| t.lesson()).flatten()
    }

    /// Checks, after every message, whether the lesson's action was done.
    pub(super) fn tutorial_progress(&mut self) -> Task<Message> {
        let Some(lesson) = self.active_lesson() else {
            return Task::none();
        };
        let Some(repo) = &self.repo else {
            return Task::none();
        };
        let Some(status) = &repo.status else {
            return Task::none();
        };
        // The state is only read once everything is up to date.
        if repo.refreshing || self.busy.is_some() {
            return Task::none();
        }
        let head = status.branch.oid.clone();
        let branches = repo.branches.iter().filter(|b| !b.remote).count();
        let redo = repo.undo.redo.len();
        let staged = status.entries.iter().any(|e| e.has_staged());
        let operation = repo.operation.is_some();
        let workspaces = repo.worktrees.list.as_ref().map_or(0, Vec::len);
        let (ahead, behind) = (status.branch.ahead, status.branch.behind);
        let upstream = status.branch.upstream.is_some();
        let section = repo.section;
        let drops = self.drops;
        let Some(t) = &mut self.tutorial else {
            return Task::none();
        };

        let Some(base) = &t.base else {
            t.base = Some(Baseline {
                head,
                branches,
                redo,
                drops,
            });
            if lesson == Lesson::Journal {
                self.journal_collapsed = false;
            }
            return match lesson.section().filter(|s| *s != section) {
                Some(section) => self.update(Message::SelectSection(section)),
                None => Task::none(),
            };
        };
        if redo > base.redo {
            t.undid = true;
        }
        if operation {
            t.saw_conflict = true;
        }
        let done = match lesson {
            Lesson::Stage => staged,
            Lesson::Commit => head != base.head,
            Lesson::Undo => t.undid && redo <= base.redo,
            Lesson::Get => head != base.head && behind == 0,
            Lesson::Send => upstream && ahead == 0,
            Lesson::Branch => branches > base.branches,
            Lesson::Drag => drops > base.drops,
            Lesson::Conflict => t.saw_conflict && !operation && head != base.head,
            Lesson::Workspace => workspaces > 1,
            Lesson::LookChange | Lesson::Tour | Lesson::Locks | Lesson::Journal => false,
        };
        if done {
            self.advance();
            // The next lesson starts from this state.
            return self.tutorial_progress();
        }
        Task::none()
    }

    // ---------------------------------------------------------------- views

    /// Frames `content` when the lesson points at it. The frame is drawn
    /// over it, so the layout never moves.
    pub(super) fn spot<'a>(
        &self,
        spot: Spot,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let lit = self.active_lesson().map(Lesson::spot) == Some(spot);
        // A sidebar item sits in a scrollable: a stack there hides every
        // layer drawn after it with the GPU renderer (the guide card among
        // them), so the frame goes around the item instead.
        if let Spot::Nav(_) = spot {
            return if lit {
                container(content).padding(2).style(style::spotlight).into()
            } else {
                content.into()
            };
        }
        let frame = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill);
        stack![
            content.into(),
            if lit {
                frame.style(style::spotlight)
            } else {
                frame
            }
        ]
        .into()
    }

    /// Dims an area of the window the lesson doesn't use.
    pub(super) fn dim<'a>(
        &self,
        region: Region,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let dimmed = self
            .active_lesson()
            .is_some_and(|l| !l.spot().lights(region));
        let veil = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill);
        stack![
            content.into(),
            if dimmed {
                veil.style(style::dimmed)
            } else {
                veil
            }
        ]
        .into()
    }

    /// The guide card of the lesson in progress.
    pub(super) fn coach_view(&self, p: &'static Palette) -> Option<Element<'_, Message>> {
        let t = self.tutorial.as_ref()?;
        let on_practice = self.repo.as_ref().is_some_and(|r| r.paths.root == t.repo);
        let total = t.track.lessons().len();
        let caption = |label: String| {
            text(label.to_uppercase())
                .size(11)
                .font(sans(Weight::Semibold))
                .color(p.accent.text)
        };
        let button_for = |label: String, message: Message, primary: bool| {
            super::modules::small_button(label, Some(message), primary)
        };
        let track = tr(&format!("tutorial.track_{}", t.track.id()));

        let (top, title, body, buttons): (String, String, String, Vec<Element<'_, Message>>) =
            if !on_practice {
                (
                    t!("tutorial.caption", track = track).into_owned(),
                    t!("tutorial.paused_title").into_owned(),
                    t!("tutorial.paused_body").into_owned(),
                    vec![
                        button_for(
                            t!("tutorial.leave").into_owned(),
                            Message::Tutorial(Msg::Leave),
                            false,
                        ),
                        button_for(
                            t!("tutorial.back").into_owned(),
                            Message::Tutorial(Msg::BackToPractice),
                            true,
                        ),
                    ],
                )
            } else if let Some(lesson) = t.lesson() {
                let key = lesson.key();
                let mut buttons = vec![button_for(
                    t!("tutorial.leave").into_owned(),
                    Message::Tutorial(Msg::Leave),
                    false,
                )];
                buttons.push(if lesson.info() {
                    button_for(
                        t!("tutorial.next").into_owned(),
                        Message::Tutorial(Msg::Next),
                        true,
                    )
                } else {
                    button_for(
                        t!("tutorial.skip").into_owned(),
                        Message::Tutorial(Msg::Skip),
                        false,
                    )
                });
                (
                    t!(
                        "tutorial.step_of",
                        track = track,
                        step = t.step + 1,
                        total = total
                    )
                    .into_owned(),
                    tr(&format!("tutorial.{key}_title")),
                    t!(
                        format!("tutorial.{key}_body"),
                        trial = practice::TRIAL_BRANCH,
                        branch = practice::CONFLICT_BRANCH
                    )
                    .into_owned(),
                    buttons,
                )
            } else {
                (
                    t!("tutorial.caption", track = track).into_owned(),
                    t!("tutorial.done_title").into_owned(),
                    t!("tutorial.done_body").into_owned(),
                    vec![
                        button_for(
                            t!("tutorial.replay").into_owned(),
                            Message::Tutorial(Msg::Offer),
                            false,
                        ),
                        button_for(
                            t!("tutorial.finish").into_owned(),
                            Message::Tutorial(Msg::Finish),
                            true,
                        ),
                    ],
                )
            };

        // Progress dots: done, current, to come.
        let mut dots = row![].spacing(4).align_y(Alignment::Center);
        for i in 0..total {
            let color = if i < t.step {
                p.accent.strong
            } else if i == t.step && on_practice {
                p.accent.text
            } else {
                p.border
            };
            dots = dots.push(
                container(Space::new())
                    .width(if i == t.step { 18 } else { 8 })
                    .height(6)
                    .style(style::dot(color)),
            );
        }

        let mut actions = row![space::horizontal()]
            .spacing(8)
            .align_y(Alignment::Center);
        for b in buttons {
            actions = actions.push(b);
        }
        let card = container(
            column![
                row![caption(top), space::horizontal(), dots]
                    .spacing(8)
                    .align_y(Alignment::Center),
                text(title).size(16).font(sans(Weight::Semibold)),
                text(body).size(13).color(p.text_2),
                actions,
            ]
            .spacing(10),
        )
        .padding(16)
        .width(380)
        .style(style::coach);
        let card = mouse_area(card).interaction(mouse::Interaction::Idle);

        // Out of the way of what the lesson points at.
        let at_top = t.lesson().map(Lesson::spot) == Some(Spot::Journal);
        let place = container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .padding(Padding::from(16).top(if at_top { 72 } else { 16 }));
        Some(
            if at_top {
                place.align_y(iced::alignment::Vertical::Top)
            } else {
                place.align_y(iced::alignment::Vertical::Bottom)
            }
            .into(),
        )
    }

    /// The track choice, shown at first launch and from the ? menu.
    pub(super) fn tutorial_offer_view(&self, p: &'static Palette) -> Element<'_, Message> {
        let mut tracks = column![].spacing(8);
        for track in Track::ALL {
            tracks = tracks.push(
                button(
                    column![
                        text(tr(&format!("tutorial.track_{}", track.id())))
                            .size(14)
                            .font(sans(Weight::Semibold)),
                        text(tr(&format!("tutorial.track_{}_hint", track.id())))
                            .size(12)
                            .color(p.text_2),
                    ]
                    .spacing(2),
                )
                .padding([10, 14])
                .width(Length::Fill)
                .on_press_maybe(self.idle().then_some(Message::Tutorial(Msg::Start(track))))
                .style(style::choice(false)),
            );
        }
        let mut buttons = row![space::horizontal()].spacing(8);
        if self.can_resume_tutorial() {
            buttons = buttons.push(super::modules::small_button(
                t!("tutorial.resume").into_owned(),
                Some(Message::Tutorial(Msg::Resume)),
                false,
            ));
        }
        buttons = buttons.push(super::modules::small_button(
            t!("tutorial.later").into_owned(),
            Some(Message::Tutorial(Msg::CloseOffer)),
            false,
        ));
        container(
            column![
                text(t!("tutorial.offer_title"))
                    .size(18)
                    .font(sans(Weight::Semibold)),
                text(t!("tutorial.offer_intro")).size(13).color(p.text_2),
                tracks,
                text(t!("tutorial.offer_hint")).size(12).color(p.muted),
                buttons,
            ]
            .spacing(12),
        )
        .padding(20)
        .max_width(520)
        .style(style::dialog)
        .into()
    }

    /// The "?" button of the top bar.
    pub(super) fn help_button(&self, p: &'static Palette) -> Element<'_, Message> {
        tooltip(
            button(container(icon(Icon::Help, 18.0, p.text_2)).center(Length::Fill))
                .padding(0)
                .width(36)
                .height(36)
                .on_press(Message::Tutorial(Msg::ToggleHelp))
                .style(style::secondary),
            container(text(t!("help.tooltip")).size(12))
                .padding([4, 8])
                .style(style::card),
            tooltip::Position::Bottom,
        )
        .into()
    }

    /// The menu of the "?" button.
    pub(super) fn help_menu(&self) -> Element<'_, Message> {
        let item = |label: String, message: Message| {
            button(text(label).size(13))
                .padding([8, 12])
                .width(Length::Fill)
                .on_press(message)
                .style(style::menu_item)
        };
        let mut menu = column![item(
            t!("tutorial.replay").into_owned(),
            Message::Tutorial(Msg::Offer)
        )];
        if self.can_resume_tutorial() {
            menu = menu.push(item(
                t!("tutorial.resume").into_owned(),
                Message::Tutorial(Msg::Resume),
            ));
        }
        if self.tutorial.is_some() {
            menu = menu.push(item(
                t!("tutorial.leave").into_owned(),
                Message::Tutorial(Msg::Leave),
            ));
        }
        let popup = container(menu.width(240)).padding(4).style(style::popup);
        opaque(
            mouse_area(
                container(popup)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .padding(Padding::default().top(54).right(220)),
            )
            .on_press(Message::Tutorial(Msg::ToggleHelp)),
        )
    }

    /// The tutorial card of the welcome screen.
    pub(super) fn tutorial_card(&self, p: &'static Palette) -> Element<'_, Message> {
        let mut buttons = row![
            button(text(t!("tutorial.start")).font(sans(Weight::Semibold)))
                .padding([10, 16])
                .on_press_maybe(self.idle().then_some(Message::Tutorial(Msg::Offer)))
                .style(style::primary),
        ]
        .spacing(10);
        if self.can_resume_tutorial() {
            buttons = buttons.push(
                button(text(t!("tutorial.resume")))
                    .padding([10, 16])
                    .on_press_maybe(self.idle().then_some(Message::Tutorial(Msg::Resume)))
                    .style(style::secondary),
            );
        }
        column![
            text(t!("tutorial.welcome_title"))
                .size(15)
                .font(sans(Weight::Semibold)),
            text(t!("tutorial.welcome_hint")).color(p.text_2),
            buttons,
        ]
        .spacing(10)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys built at runtime exist in every language.
    #[test]
    fn lesson_texts_exist() {
        let mut keys = Vec::new();
        for track in Track::ALL {
            keys.push(format!("tutorial.track_{}", track.id()));
            keys.push(format!("tutorial.track_{}_hint", track.id()));
            for lesson in track.lessons() {
                keys.push(format!("tutorial.{}_title", lesson.key()));
                keys.push(format!("tutorial.{}_body", lesson.key()));
            }
        }
        for key in keys {
            for locale in ["en", "fr"] {
                let text = t!(&key, locale = locale);
                assert!(!text.contains(&key), "missing {key} in {locale}");
            }
        }
    }

    #[test]
    fn tracks_round_trip_and_info_lessons_point_somewhere() {
        for track in Track::ALL {
            assert_eq!(Track::from_id(track.id()), Some(track));
            assert!(!track.lessons().is_empty());
            // Every track ends on the Git log.
            assert_eq!(track.lessons().last(), Some(&Lesson::Journal));
            for lesson in track.lessons() {
                if let Some(Section::Locks | Section::Workspaces) = lesson.section() {
                    assert!(matches!(lesson.spot(), Spot::Nav(_)));
                }
            }
        }
    }
}
