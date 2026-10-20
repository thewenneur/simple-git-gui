// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate rust_i18n;

mod app;
mod git;
mod i18n;

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
    .subscription(App::subscription)
    .window_size((1100.0, 760.0))
    .run()
}
