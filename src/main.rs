// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate rust_i18n;

mod app;
mod git;
mod i18n;
mod settings;
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
    .window(window_settings())
    .run()
}

fn window_settings() -> iced::window::Settings {
    #[allow(unused_mut)]
    let mut settings = iced::window::Settings {
        size: iced::Size::new(1280.0, 800.0),
        icon: window_icon(),
        ..iced::window::Settings::default()
    };
    // Matches the .desktop file name, so Wayland docks show the right icon.
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = APP_ID.to_string();
    }
    settings
}

/// Also the macOS bundle identifier and the Linux .desktop file name.
#[cfg(target_os = "linux")]
const APP_ID: &str = "io.github.thewenneur.simple-git-gui";

/// The title bar and taskbar icon. Executables also carry the icon on Windows
/// (`build.rs`) and in the macOS bundle; this one is for the running window.
fn window_icon() -> Option<iced::window::Icon> {
    let png = include_bytes!("../assets/icon/simple-git-64.png");
    let decoder = png::Decoder::new(&png[..]);
    let mut reader = decoder.read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    rgba.truncate(info.buffer_size());
    iced::window::icon::from_rgba(rgba, info.width, info.height).ok()
}

/// Picks the renderer, unless `ICED_BACKEND` is set. `SIMPLE_GIT_GPU=1` or `0`
/// forces the GPU (wgpu) or the CPU (tiny-skia). By default:
/// - on Windows and Linux, the CPU: the GPU renderer loads the graphics driver,
///   about 120 MB more than this flat UI needs;
/// - on macOS, the GPU: Metal is cheap there, while the CPU renderer keeps
///   several full-size Retina frames in memory (160 MB full screen).
fn choose_renderer() {
    if std::env::var_os("ICED_BACKEND").is_some() {
        return;
    }
    let gpu = match std::env::var("SIMPLE_GIT_GPU").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => cfg!(target_os = "macos"),
    };
    let backend = if gpu { "wgpu,tiny-skia" } else { "tiny-skia" };
    // SAFETY: called first thing in `main`, before any other thread exists.
    unsafe { std::env::set_var("ICED_BACKEND", backend) };
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_icon_decodes() {
        assert!(super::window_icon().is_some());
    }
}
