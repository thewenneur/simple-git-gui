//! Colors, fonts and widget styles of the approved design.
//!
//! The light and dark palettes come from the validated wireframes. Style
//! functions pick the palette from the active iced [`Theme`], so the same
//! view code works in both.

use iced::font::Weight;
use iced::overlay::menu;
use iced::widget::{button, checkbox, container, pick_list, scrollable, text_editor, text_input};
use iced::{Background, Border, Color, Font, Shadow, Theme, color};

pub const SANS: Font = Font::with_name("IBM Plex Sans");
pub const MONO: Font = Font::with_name("IBM Plex Mono");

pub const FONT_FILES: [&[u8]; 5] = [
    include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexMono-Medium.ttf"),
];

pub fn weight(font: Font, weight: Weight) -> Font {
    Font { weight, ..font }
}

pub fn sans(w: Weight) -> Font {
    weight(SANS, w)
}

pub fn mono(w: Weight) -> Font {
    weight(MONO, w)
}

pub const RADIUS: f32 = 6.0;

/// A color family used for one meaning (branch kind, diff state...).
#[derive(Debug, Clone, Copy)]
pub struct Tint {
    /// Dot, icon or strong text.
    pub strong: Color,
    /// Background of a highlighted item.
    pub soft: Color,
    /// Text on the soft background.
    pub text: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub page: Color,
    pub surface: Color,
    pub sidebar: Color,
    pub border: Color,
    pub border_soft: Color,
    pub input_border: Color,
    pub text: Color,
    pub text_2: Color,
    pub muted: Color,
    pub disabled: Color,
    pub accent: Tint,
    pub accent_hover: Color,
    pub nav_active_bg: Color,
    pub nav_active_border: Color,
    pub added: Tint,
    pub removed: Tint,
    /// Lines moved elsewhere in the file (same content).
    pub moved: Tint,
    pub warning: Tint,
    /// Background of card footers (commit form).
    pub surface_alt: Color,
    pub line_number: Color,
    pub branch_main: Tint,
    pub branch_feature: Tint,
    pub branch_fix: Tint,
    pub branch_other: Tint,
    pub journal_bg: Color,
    pub journal_border: Color,
}

pub const LIGHT: Palette = Palette {
    page: color!(0xF3F5F8),
    surface: color!(0xFFFFFF),
    sidebar: color!(0xE9EDF2),
    border: color!(0xD5DBE3),
    border_soft: color!(0xE3E7ED),
    input_border: color!(0xB9C2CE),
    text: color!(0x18202B),
    text_2: color!(0x4A5566),
    muted: color!(0x5E6878),
    disabled: color!(0xA9B2BF),
    accent: Tint {
        strong: color!(0x2450C8),
        soft: color!(0xE4EBFB),
        text: color!(0x1A3C99),
    },
    accent_hover: color!(0x1A3C99),
    nav_active_bg: color!(0xFFFFFF),
    nav_active_border: color!(0xD5DBE3),
    added: Tint {
        strong: color!(0x17633F),
        soft: color!(0xE3F3EA),
        text: color!(0x0F4A2E),
    },
    removed: Tint {
        strong: color!(0xA3261B),
        soft: color!(0xFBE9E7),
        text: color!(0x7A1B12),
    },
    moved: Tint {
        strong: color!(0x5534B8),
        soft: color!(0xEEEAFB),
        text: color!(0x3C2490),
    },
    surface_alt: color!(0xFAFBFC),
    line_number: color!(0x8893A3),
    warning: Tint {
        strong: color!(0x8A5300),
        soft: color!(0xFFF3D6),
        text: color!(0x8A5300),
    },
    branch_main: Tint {
        strong: color!(0x2450C8),
        soft: color!(0xE4EBFB),
        text: color!(0x1A3C99),
    },
    branch_feature: Tint {
        strong: color!(0xC2570C),
        soft: color!(0xFDF1E7),
        text: color!(0x8F3E06),
    },
    branch_fix: Tint {
        strong: color!(0x7B3FC4),
        soft: color!(0xF1EAFB),
        text: color!(0x5A2A99),
    },
    branch_other: Tint {
        strong: color!(0x0F7B74),
        soft: color!(0xE2F4F2),
        text: color!(0x0B5A55),
    },
    journal_bg: color!(0x141A23),
    journal_border: color!(0x263040),
};

pub const DARK: Palette = Palette {
    page: color!(0x11161E),
    surface: color!(0x19202B),
    sidebar: color!(0x151B24),
    border: color!(0x2A3443),
    border_soft: color!(0x222B38),
    input_border: color!(0x3A4659),
    text: color!(0xE6EBF2),
    text_2: color!(0xA9B4C4),
    muted: color!(0x8D99AB),
    disabled: color!(0x6B778A),
    accent: Tint {
        strong: color!(0x3D6BE0),
        soft: color!(0x1E2A45),
        text: color!(0x9DB8FF),
    },
    accent_hover: color!(0x5B85F0),
    nav_active_bg: color!(0x1E2A45),
    nav_active_border: color!(0x34487A),
    added: Tint {
        strong: color!(0x7FD6A4),
        soft: color!(0x12301F),
        text: color!(0xBDEBD0),
    },
    removed: Tint {
        strong: color!(0xF2A196),
        soft: color!(0x3A1A17),
        text: color!(0xF8C7BF),
    },
    moved: Tint {
        strong: color!(0xBFAEFF),
        soft: color!(0x2A2147),
        text: color!(0xDCD2FF),
    },
    surface_alt: color!(0x161C26),
    line_number: color!(0x6B778A),
    warning: Tint {
        strong: color!(0xF0C674),
        soft: color!(0x3A2E12),
        text: color!(0xF0C674),
    },
    branch_main: Tint {
        strong: color!(0x5B85F0),
        soft: color!(0x1E2A45),
        text: color!(0x9DB8FF),
    },
    branch_feature: Tint {
        strong: color!(0xE8803A),
        soft: color!(0x2E1D11),
        text: color!(0xF5A262),
    },
    branch_fix: Tint {
        strong: color!(0xA77BEA),
        soft: color!(0x2A2147),
        text: color!(0xDCD2FF),
    },
    branch_other: Tint {
        strong: color!(0x4FC1B8),
        soft: color!(0x13302D),
        text: color!(0xA6E3DE),
    },
    journal_bg: color!(0x0B0F15),
    journal_border: color!(0x1E2633),
};

/// Colors of the Git journal, identical in both themes (it is always dark).
pub mod journal {
    use iced::{Color, color};
    pub const TEXT: Color = color!(0xD8DEE8);
    pub const TITLE: Color = color!(0xFFFFFF);
    pub const HINT: Color = color!(0x9AA6B8);
    pub const OK: Color = color!(0x6FCF97);
    pub const FAIL: Color = color!(0xF2A196);
    pub const PROMPT: Color = color!(0x8FB3FF);
    pub const DURATION: Color = color!(0x7D8899);
    pub const BUTTON_BORDER: Color = color!(0x3A4659);
}

pub fn theme(dark: bool) -> Theme {
    let p = if dark { DARK } else { LIGHT };
    Theme::custom(
        if dark {
            "Simple Git (sombre)"
        } else {
            "Simple Git"
        },
        iced::theme::Palette {
            background: p.page,
            text: p.text,
            primary: p.accent.strong,
            success: p.added.strong,
            warning: p.warning.strong,
            danger: p.removed.strong,
        },
    )
}

pub fn palette(theme: &Theme) -> &'static Palette {
    if theme.extended_palette().is_dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// Colors of a branch, from the usual naming conventions.
pub fn branch_tint(p: &Palette, name: &str) -> Tint {
    let local = name.split_once('/').map_or(name, |(first, rest)| {
        // `origin/feature/x` → `feature/x`
        if rest.contains('/') || matches!(first, "origin" | "upstream") {
            rest
        } else {
            name
        }
    });
    let lower = local.to_lowercase();
    if matches!(lower.as_str(), "main" | "master" | "develop" | "dev") {
        p.branch_main
    } else if ["feature/", "feat/"].iter().any(|s| lower.starts_with(s)) {
        p.branch_feature
    } else if ["fix/", "hotfix/", "bugfix/"]
        .iter()
        .any(|s| lower.starts_with(s))
    {
        p.branch_fix
    } else {
        p.branch_other
    }
}

fn border(color: Color, width: f32, radius: f32) -> Border {
    Border {
        color,
        width,
        radius: radius.into(),
    }
}

fn boxed(background: Color, border_color: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(background.into()),
        border: border(border_color, 1.0, radius),
        ..container::Style::default()
    }
}

// ---- containers ----

pub fn page(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.page.into()),
        text_color: Some(p.text),
        ..container::Style::default()
    }
}

/// Top bar: surface with a bottom border (drawn by a rule under it).
pub fn header(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.surface.into()),
        ..container::Style::default()
    }
}

pub fn sidebar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.sidebar.into()),
        ..container::Style::default()
    }
}

pub fn card(theme: &Theme) -> container::Style {
    let p = palette(theme);
    boxed(p.surface, p.border, RADIUS)
}

pub fn separator(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(palette(theme).border_soft.into()),
        ..container::Style::default()
    }
}

pub fn strong_separator(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(palette(theme).border.into()),
        ..container::Style::default()
    }
}

pub fn journal(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.journal_bg.into()),
        text_color: Some(journal::TEXT),
        ..container::Style::default()
    }
}

/// A dark box like the journal, for a command line.
pub fn journal_box(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        text_color: Some(journal::TEXT),
        ..boxed(p.journal_bg, p.journal_bg, RADIUS)
    }
}

pub fn journal_separator(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(palette(theme).journal_border.into()),
        ..container::Style::default()
    }
}

pub fn tinted(tint: Tint) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(tint.soft.into()),
        text_color: Some(tint.text),
        border: border(Color::TRANSPARENT, 0.0, RADIUS),
        ..container::Style::default()
    }
}

/// A small rounded label (badge, ref name).
pub fn badge(background: Color, text: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(background.into()),
        text_color: Some(text),
        border: border(Color::TRANSPARENT, 0.0, 10.0),
        ..container::Style::default()
    }
}

pub fn dot(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(color.into()),
        border: border(Color::TRANSPARENT, 0.0, 5.0),
        ..container::Style::default()
    }
}

pub fn banner_error(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        text_color: Some(p.removed.text),
        ..boxed(p.removed.soft, p.removed.strong, RADIUS)
    }
}

pub fn banner_info(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        text_color: Some(p.accent.text),
        ..boxed(p.accent.soft, p.accent.soft, RADIUS)
    }
}

pub fn banner_warning(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        text_color: Some(p.warning.text),
        ..boxed(p.warning.soft, p.warning.strong, RADIUS)
    }
}

/// A highlighted row (selected file): soft accent with a bar on the left.
pub fn selected_row(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.accent.soft.into()),
        ..container::Style::default()
    }
}

/// A history row a drag would drop on.
pub fn drop_target(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.accent.soft.into()),
        border: border(p.accent.strong, 2.0, 4.0),
        ..container::Style::default()
    }
}

pub fn accent_bar(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(palette(theme).accent.strong.into()),
        ..container::Style::default()
    }
}

/// Footer of a card, slightly off the surface color.
pub fn card_footer(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.surface_alt.into()),
        border: Border {
            radius: iced::border::Radius::default()
                .bottom_left(RADIUS)
                .bottom_right(RADIUS),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Header of a diff hunk.
pub fn hunk_header(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(palette(theme).page.into()),
        ..container::Style::default()
    }
}

/// Background of a diff line.
pub fn line(background: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(background.into()),
        ..container::Style::default()
    }
}

/// A legend swatch: soft fill with a strong border.
pub fn swatch(tint: Tint) -> impl Fn(&Theme) -> container::Style {
    move |_| boxed(tint.soft, tint.strong, 3.0)
}

/// The dimmed backdrop behind a dialog.
pub fn backdrop(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba8(0x10, 0x16, 0x20, 0.55).into()),
        ..container::Style::default()
    }
}

pub fn dialog(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        text_color: Some(p.text),
        ..boxed(p.surface, p.border, 8.0)
    }
}

/// A badge with only an outline (remote branches).
/// A box with a colored outline on the card's surface.
pub fn outlined_box(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |theme| boxed(palette(theme).surface, color, RADIUS)
}

pub fn outlined_badge(tint: Tint) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: Some(palette(theme).surface.into()),
        text_color: Some(tint.text),
        border: border(tint.strong, 1.0, 10.0),
        ..container::Style::default()
    }
}

/// The "you are here" badge of a detached HEAD (iced draws no dashes, so a
/// strong outline on the soft color stands for the dashed border).
pub fn dashed_badge(tint: Tint) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(tint.soft.into()),
        text_color: Some(tint.text),
        border: border(tint.strong, 1.0, 10.0),
        ..container::Style::default()
    }
}

/// A floating menu (right-click).
pub fn popup(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        text_color: Some(p.text),
        shadow: Shadow {
            color: Color::from_rgba8(0x18, 0x20, 0x2B, 0.18),
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..boxed(p.surface, p.input_border, 8.0)
    }
}

/// A step of the conflict assistant's header: done, current or to come.
pub fn wizard_step(state: std::cmp::Ordering) -> impl Fn(&Theme) -> container::Style {
    move |_| match state {
        std::cmp::Ordering::Less => container::Style {
            background: Some(color!(0x263040).into()),
            text_color: Some(color!(0x9FE0BC)),
            border: border(Color::TRANSPARENT, 0.0, 14.0),
            ..container::Style::default()
        },
        std::cmp::Ordering::Equal => container::Style {
            background: Some(Color::WHITE.into()),
            text_color: Some(color!(0x18202B)),
            border: border(Color::TRANSPARENT, 0.0, 14.0),
            ..container::Style::default()
        },
        std::cmp::Ordering::Greater => container::Style {
            text_color: Some(journal::HINT),
            border: border(journal::BUTTON_BORDER, 1.0, 14.0),
            ..container::Style::default()
        },
    }
}

/// Header of one side of a conflict, in its branch's color.
pub fn side_header(tint: Tint) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(tint.soft.into()),
        border: Border {
            color: tint.strong,
            width: 0.0,
            radius: 0.0.into(),
        },
        shadow: Shadow {
            color: tint.strong,
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 0.0,
        },
        ..container::Style::default()
    }
}

/// Numbered step marker of a wizard.
pub fn step_marker(state: StepState) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        match state {
            StepState::Done => container::Style {
                text_color: Some(Color::WHITE),
                ..boxed(p.added.strong, p.added.strong, 14.0)
            },
            StepState::Current => container::Style {
                text_color: Some(Color::WHITE),
                ..boxed(p.accent.strong, p.accent.strong, 14.0)
            },
            StepState::Failed => container::Style {
                text_color: Some(Color::WHITE),
                ..boxed(p.removed.strong, p.removed.strong, 14.0)
            },
            StepState::Todo => container::Style {
                text_color: Some(p.muted),
                background: None,
                border: border(p.input_border, 2.0, 14.0),
                ..container::Style::default()
            },
        }
    }
}

/// Outer ring of a radio-like choice.
pub fn radio(selected: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        let ring = if selected {
            p.accent.strong
        } else {
            p.input_border
        };
        container::Style {
            background: Some(p.surface.into()),
            border: border(ring, 1.5, 8.0),
            ..container::Style::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    Done,
    Current,
    Failed,
    Todo,
}

// ---- buttons ----

fn button_base(
    background: Option<Color>,
    text: Color,
    border_color: Color,
    width: f32,
) -> button::Style {
    button::Style {
        background: background.map(Background::Color),
        text_color: text,
        border: border(border_color, width, RADIUS),
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    match status {
        button::Status::Disabled => {
            button_base(Some(p.border_soft), p.disabled, Color::TRANSPARENT, 0.0)
        }
        button::Status::Hovered | button::Status::Pressed => {
            button_base(Some(p.accent_hover), Color::WHITE, Color::TRANSPARENT, 0.0)
        }
        button::Status::Active => {
            button_base(Some(p.accent.strong), Color::WHITE, Color::TRANSPARENT, 0.0)
        }
    }
}

pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    match status {
        button::Status::Disabled => button_base(Some(p.surface), p.disabled, p.border_soft, 1.0),
        button::Status::Hovered | button::Status::Pressed => {
            button_base(Some(p.page), p.text, p.input_border, 1.0)
        }
        button::Status::Active => button_base(Some(p.surface), p.text, p.border, 1.0),
    }
}

/// The repository selector in the top bar.
pub fn repo_pill(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let border_color = if matches!(status, button::Status::Hovered) {
        p.input_border
    } else {
        p.border
    };
    button_base(Some(p.page), p.text, border_color, 1.0)
}

/// Current branch indicator in the top bar.
pub fn branch_pill(tint: Tint) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: Some(tint.soft.into()),
        text_color: tint.text,
        border: border(tint.strong.scale_alpha(0.35), 1.0, 18.0),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Sidebar navigation entry.
pub fn nav(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        if active {
            button_base(Some(p.nav_active_bg), p.text, p.nav_active_border, 1.0)
        } else if matches!(status, button::Status::Hovered) {
            button_base(Some(p.border_soft), p.text, Color::TRANSPARENT, 1.0)
        } else {
            button_base(None, p.text, Color::TRANSPARENT, 1.0)
        }
    }
}

/// One half of a segmented control (language, view mode).
pub fn segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let mut style = if selected {
            button_base(Some(p.text), p.surface, Color::TRANSPARENT, 0.0)
        } else if matches!(status, button::Status::Hovered) {
            button_base(Some(p.page), p.text, Color::TRANSPARENT, 0.0)
        } else {
            button_base(Some(p.surface), p.text_2, Color::TRANSPARENT, 0.0)
        };
        style.border.radius = 0.0.into();
        style
    }
}

/// A selectable option card (radio-like choice in a wizard).
pub fn choice(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        if selected {
            button_base(Some(p.accent.soft), p.text, p.accent.strong, 1.0)
        } else if matches!(status, button::Status::Hovered) {
            button_base(Some(p.page), p.text, p.input_border, 1.0)
        } else {
            button_base(None, p.text, p.border, 1.0)
        }
    }
}

/// A segment that is filled with the accent color when selected (scope).
pub fn accent_segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let mut style = if selected {
            button_base(Some(p.accent.strong), Color::WHITE, Color::TRANSPARENT, 0.0)
        } else if matches!(status, button::Status::Hovered) {
            button_base(Some(p.page), p.text, Color::TRANSPARENT, 0.0)
        } else {
            button_base(Some(p.surface), p.text_2, Color::TRANSPARENT, 0.0)
        };
        style.border.radius = 0.0.into();
        style
    }
}

/// A light selection segment (Unified / Side by side), as in the wireframes.
pub fn soft_segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let mut style = if selected {
            button_base(Some(p.accent.soft), p.accent.text, Color::TRANSPARENT, 0.0)
        } else if matches!(status, button::Status::Hovered) {
            button_base(Some(p.page), p.text, Color::TRANSPARENT, 0.0)
        } else {
            button_base(Some(p.surface), p.text_2, Color::TRANSPARENT, 0.0)
        };
        style.border.radius = 0.0.into();
        style
    }
}

/// An entry of a right-click menu.
pub fn menu_item(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let mut style = match status {
        button::Status::Disabled => button_base(None, p.disabled, Color::TRANSPARENT, 0.0),
        button::Status::Hovered | button::Status::Pressed => {
            button_base(Some(p.accent.soft), p.text, Color::TRANSPARENT, 0.0)
        }
        button::Status::Active => button_base(None, p.text, Color::TRANSPARENT, 0.0),
    };
    style.border.radius = 4.0.into();
    style
}

/// A button that does something hard to take back (delete for good).
pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    match status {
        button::Status::Disabled => {
            button_base(Some(p.border_soft), p.disabled, Color::TRANSPARENT, 0.0)
        }
        button::Status::Hovered | button::Status::Pressed => {
            button_base(Some(p.removed.text), Color::WHITE, Color::TRANSPARENT, 0.0)
        }
        button::Status::Active => button_base(
            Some(p.removed.strong),
            Color::WHITE,
            Color::TRANSPARENT,
            0.0,
        ),
    }
}

/// The fix button of a warning banner.
pub fn warning_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    match status {
        button::Status::Disabled => {
            button_base(Some(p.border_soft), p.disabled, Color::TRANSPARENT, 0.0)
        }
        _ => button_base(
            Some(p.warning.strong),
            Color::WHITE,
            Color::TRANSPARENT,
            0.0,
        ),
    }
}

/// "Keep this version" under one side of a conflict.
pub fn side_button(tint: Tint, picked: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        if picked {
            button_base(Some(tint.strong), Color::WHITE, tint.strong, 1.0)
        } else if matches!(status, button::Status::Hovered) {
            button_base(Some(tint.soft), tint.text, tint.strong, 1.0)
        } else {
            button_base(Some(p.surface), tint.text, tint.strong, 1.0)
        }
    }
}

/// Plain text button (links, close).
pub fn link(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let text = if matches!(status, button::Status::Hovered) {
        p.accent_hover
    } else {
        p.accent.strong
    };
    button_base(None, text, Color::TRANSPARENT, 0.0)
}

/// Buttons inside the dark journal.
pub fn journal_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = matches!(status, button::Status::Hovered)
        .then_some(journal::BUTTON_BORDER.scale_alpha(0.4));
    button_base(background, journal::TEXT, journal::BUTTON_BORDER, 1.0)
}

/// Container outlining a segmented control.
pub fn segmented_frame(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        border: border(p.border, 1.0, RADIUS),
        ..container::Style::default()
    }
}

// ---- inputs ----

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = palette(theme);
    let border_color = match status {
        text_input::Status::Focused { .. } => p.accent.strong,
        text_input::Status::Hovered => p.text_2,
        _ => p.input_border,
    };
    text_input::Style {
        background: p.surface.into(),
        border: border(border_color, 1.0, RADIUS),
        icon: p.muted,
        placeholder: p.muted,
        value: p.text,
        selection: p.accent.soft,
    }
}

pub fn pick(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let p = palette(theme);
    let border_color = match status {
        pick_list::Status::Opened { .. } => p.accent.strong,
        pick_list::Status::Hovered => p.text_2,
        pick_list::Status::Active => p.input_border,
    };
    pick_list::Style {
        text_color: p.text,
        placeholder_color: p.muted,
        handle_color: p.text_2,
        background: p.surface.into(),
        border: border(border_color, 1.0, RADIUS),
    }
}

pub fn menu(theme: &Theme) -> menu::Style {
    let p = palette(theme);
    menu::Style {
        background: p.surface.into(),
        border: border(p.border, 1.0, RADIUS),
        text_color: p.text,
        selected_text_color: p.accent.text,
        selected_background: p.accent.soft.into(),
        shadow: Shadow::default(),
    }
}

pub fn editor(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let p = palette(theme);
    let border_color = match status {
        text_editor::Status::Focused { .. } => p.accent.strong,
        text_editor::Status::Hovered => p.text_2,
        _ => p.input_border,
    };
    text_editor::Style {
        background: p.surface.into(),
        border: border(border_color, 1.0, RADIUS),
        placeholder: p.muted,
        value: p.text,
        selection: p.accent.soft,
    }
}

pub fn journal_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused { .. } => journal::PROMPT,
        _ => journal::BUTTON_BORDER,
    };
    text_input::Style {
        background: Color::TRANSPARENT.into(),
        border: border(border_color, 1.0, RADIUS),
        icon: journal::HINT,
        placeholder: journal::DURATION,
        value: journal::TEXT,
        selection: journal::BUTTON_BORDER,
    }
}

pub fn check(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let p = palette(theme);
    let checked = match status {
        checkbox::Status::Active { is_checked }
        | checkbox::Status::Hovered { is_checked }
        | checkbox::Status::Disabled { is_checked } => is_checked,
    };
    checkbox::Style {
        background: if checked { p.accent.strong } else { p.surface }.into(),
        icon_color: Color::WHITE,
        border: border(
            if checked {
                p.accent.strong
            } else {
                p.input_border
            },
            1.0,
            3.0,
        ),
        text_color: Some(p.text_2),
    }
}

pub fn journal_check(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    checkbox::Style {
        text_color: Some(journal::HINT),
        ..check(theme, status)
    }
}

pub fn scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let p = palette(theme);
    let mut style = scrollable::default(theme, status);
    for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
        rail.background = None;
        rail.border = Border::default();
        rail.scroller.border = border(Color::TRANSPARENT, 0.0, 4.0);
        if !matches!(
            status,
            scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
        ) {
            rail.scroller.background = p.border.into();
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_colors_follow_naming() {
        let p = &LIGHT;
        assert_eq!(branch_tint(p, "main").strong, p.branch_main.strong);
        assert_eq!(branch_tint(p, "origin/main").strong, p.branch_main.strong);
        assert_eq!(
            branch_tint(p, "feature/uart-rx").strong,
            p.branch_feature.strong
        );
        assert_eq!(
            branch_tint(p, "origin/feature/uart-rx").strong,
            p.branch_feature.strong
        );
        assert_eq!(branch_tint(p, "fix/timing-pll").strong, p.branch_fix.strong);
        assert_eq!(branch_tint(p, "essai").strong, p.branch_other.strong);
    }

    #[test]
    fn themes_are_detected_as_dark_or_light() {
        assert_eq!(palette(&theme(false)).page, LIGHT.page);
        assert_eq!(palette(&theme(true)).page, DARK.page);
    }
}
