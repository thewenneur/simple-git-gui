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
    choose_renderer();
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

/// Draws with the CPU unless `SIMPLE_GIT_GPU=1`: the GPU renderer loads the
/// graphics driver, which costs far more memory than this flat UI needs.
/// `ICED_BACKEND`, when set, still wins.
fn choose_renderer() {
    if std::env::var_os("ICED_BACKEND").is_some() {
        return;
    }
    let gpu = std::env::var("SIMPLE_GIT_GPU").is_ok_and(|v| v == "1");
    let backend = if gpu { "wgpu,tiny-skia" } else { "tiny-skia" };
    // SAFETY: called first thing in `main`, before any other thread exists.
    unsafe { std::env::set_var("ICED_BACKEND", backend) };
}
