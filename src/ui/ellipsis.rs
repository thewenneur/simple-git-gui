//! One line of text that ends with "…" when it does not fit, instead of being
//! cut in the middle of a letter.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph, paragraph::Plain};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::{Color, Element, Font, Length, Pixels, Point, Rectangle, Size, alignment, mouse};

const DOTS: &str = "…";

pub struct Ellipsis {
    content: String,
    size: Option<Pixels>,
    font: Option<Font>,
    color: Option<Color>,
}

/// A single line of `content`, shortened with "…" to the width it is given.
pub fn ellipsis(content: impl Into<String>) -> Ellipsis {
    Ellipsis {
        content: content.into(),
        size: None,
        font: None,
        color: None,
    }
}

impl Ellipsis {
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = Some(size.into());
        self
    }

    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

#[derive(Default)]
struct State<P: Paragraph> {
    full: Plain<P>,
    /// The shortened line, when the full one does not fit.
    shown: Option<Plain<P>>,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Ellipsis
where
    Renderer: text::Renderer<Font = Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer::Paragraph>::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();
        let max = limits.max();
        let size = self.size.unwrap_or_else(|| renderer.default_size());
        let font = self.font.unwrap_or_else(|| renderer.default_font());
        let line = text::Text {
            content: (),
            bounds: Size::new(f32::INFINITY, max.height),
            size,
            line_height: text::LineHeight::default(),
            font,
            align_x: text::Alignment::Default,
            align_y: alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        };

        state.full.update(line.with_content(self.content.as_str()));
        let full = state.full.min_bounds();
        if full.width <= max.width {
            state.shown = None;
            // Drawn text needs finite bounds (the renderer tracks what changed).
            state.full.update(text::Text {
                bounds: full,
                ..line.with_content(self.content.as_str())
            });
            return layout::Node::new(full);
        }

        let dots = Renderer::Paragraph::with_text(line.with_content(DOTS)).min_width();
        let target = (max.width - dots).max(0.0);
        let mut end = state
            .full
            .raw()
            .hit_test(Point::new(target, full.height / 2.0))
            .map(text::Hit::cursor)
            .unwrap_or(0)
            .min(self.content.len());
        while !self.content.is_char_boundary(end) {
            end -= 1;
        }
        let shown = state.shown.get_or_insert_with(Plain::default);
        // The hit can land on the far half of a letter: step back until it fits.
        loop {
            let cut = format!("{}{DOTS}", self.content[..end].trim_end());
            shown.update(line.with_content(cut.as_str()));
            if shown.min_width() <= max.width || end == 0 {
                break;
            }
            end = self.content[..end]
                .char_indices()
                .last()
                .map_or(0, |(i, _)| i);
        }
        let fitted = Size::new(shown.min_width().min(max.width), shown.min_height());
        let cut = shown.content().to_string();
        shown.update(text::Text {
            bounds: fitted,
            ..line.with_content(cut.as_str())
        });
        layout::Node::new(fitted)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        let paragraph = state.shown.as_ref().unwrap_or(&state.full);
        renderer.fill_paragraph(
            paragraph.raw(),
            layout.bounds().position(),
            self.color.unwrap_or(style.text_color),
            *viewport,
        );
    }
}

impl<'a, Message, Theme, Renderer> From<Ellipsis> for Element<'a, Message, Theme, Renderer>
where
    Renderer: text::Renderer<Font = Font> + 'a,
{
    fn from(widget: Ellipsis) -> Self {
        Element::new(widget)
    }
}
