// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate rust_i18n;

mod app;
mod git;
mod i18n;
mod ui;

i18n!("locales", fallback = "en");

use std::path::PathBuf;

use app::App;

fn main() -> iced::Result {
    let initial_dir = std::env::args_os().nth(1).map(PathBuf::from);
    iced::application(
        move || App::new(initial_dir.clone()),
        App::update,
        App::view,
    )
    .title(App::title)
    .theme(App::theme)
    .subscription(App::subscription)
    .settings(iced::Settings {
        fonts: ui::style::FONT_FILES.iter().map(|f| (*f).into()).collect(),
        default_font: ui::style::SANS,
        default_text_size: 14.into(),
        ..iced::Settings::default()
    })
    .window_size((1280.0, 800.0))
    .run()
}
