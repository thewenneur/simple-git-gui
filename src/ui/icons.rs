//! Stroke icons from the approved design, tinted at draw time.

use iced::widget::svg;
use iced::{Color, Element, Length};

#[derive(Debug, Clone, Copy)]
pub enum Icon {
    Logo,
    Folder,
    ChevronDown,
    ChevronUp,
    Changes,
    History,
    BranchSmall,
    Check,
    Cross,
}

impl Icon {
    fn source(self) -> &'static str {
        match self {
            Icon::Logo => {
                r#"<circle cx="6" cy="5" r="2.5"/><circle cx="6" cy="19" r="2.5"/><circle cx="18" cy="9" r="2.5"/><path d="M6 7.5v9M18 11.5c0 4-6 3-11 6"/>"#
            }
            Icon::Folder => r#"<path d="M3 6h7l2 2h9v11H3z"/>"#,
            Icon::ChevronDown => r#"<path d="M6 9l6 6 6-6"/>"#,
            Icon::ChevronUp => r#"<path d="M6 15l6-6 6 6"/>"#,
            Icon::Changes => r#"<path d="M5 3h10l4 4v14H5z"/><path d="M9 13h6M12 10v6"/>"#,
            Icon::History => {
                r#"<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="12" r="2"/><path d="M6 7v10M6 9c0 3 6 3 10 3"/>"#
            }
            Icon::Check => r#"<path d="M5 12l5 5 9-10"/>"#,
            Icon::Cross => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
            Icon::BranchSmall => {
                r#"<circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><path d="M6 8.5v7"/>"#
            }
        }
    }

    fn handle(self) -> svg::Handle {
        let doc = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
            self.source()
        );
        svg::Handle::from_memory(doc.into_bytes())
    }
}

pub fn icon<'a, Message: 'a>(icon: Icon, size: f32, color: Color) -> Element<'a, Message> {
    svg(icon.handle())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}
