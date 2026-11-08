//! Studio design system; `brand.md` at the repository root is the reference. Monochrome tokens (black
//! ground, white text, #b3b3b3 muted, #404040 borders, #222 surfaces), Helvetica-style type,
//! 12 px cards, 8 px controls at least 44 px tall, filled primary action, 4 px spacing grid.
//!
//! Rules: one neutral scale for surfaces/borders/text, semantic colors only for state
//! (success/warning/danger/info), 4px spacing grid, radius 6 (controls) / 12 (cards),
//! one accent per element. Components here are the only place colors are chosen.
use gpui::{prelude::*, *};

pub const FONT: &str = if cfg!(target_os = "macos") { "Helvetica Neue" } else { "Nimbus Sans" };
pub const MONO: &str = if cfg!(target_os = "macos") { "Menlo" } else { "JetBrainsMono Nerd Font" };

pub mod color {
    use gpui::{Rgba, rgb};
    // Neutral scale (zinc).
    pub fn background() -> Rgba {
        rgb(0x000000)
    }
    pub fn card() -> Rgba {
        rgb(0x0d0d0d)
    }
    pub fn muted() -> Rgba {
        rgb(0x222222)
    }
    pub fn accent() -> Rgba {
        rgb(0x2e2e2e)
    }
    pub fn border() -> Rgba {
        rgb(0x404040)
    }
    /// Edge of operable controls; `border` is too faint for them (3:1 non-text contrast).
    pub fn input() -> Rgba {
        rgb(0x8c8c8c)
    }
    pub fn foreground() -> Rgba {
        rgb(0xfafafa)
    }
    pub fn muted_foreground() -> Rgba {
        rgb(0xb3b3b3)
    }
    pub fn subtle_foreground() -> Rgba {
        rgb(0x8c8c8c)
    }
    // Semantic.
    pub fn success() -> Rgba {
        rgb(0x22c55e)
    }
    pub fn warning() -> Rgba {
        rgb(0xf59e0b)
    }
    pub fn danger() -> Rgba {
        rgb(0xef4444)
    }
    pub fn danger_solid() -> Rgba {
        rgb(0xdc2626)
    }
    pub fn danger_hover() -> Rgba {
        rgb(0xb91c1c)
    }
    pub fn info() -> Rgba {
        rgb(0x60a5fa)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Success,
    Warning,
    Danger,
    Info,
}

impl Tone {
    pub fn color(self) -> Rgba {
        match self {
            Tone::Neutral => color::muted_foreground(),
            Tone::Success => color::success(),
            Tone::Warning => color::warning(),
            Tone::Danger => color::danger(),
            Tone::Info => color::info(),
        }
    }
}

fn tint(c: Rgba, alpha: f32) -> Rgba {
    Rgba { a: alpha, ..c }
}

/// Bordered surface with a header (title + optional description) and body.
pub fn card(title: &str, description: Option<&str>) -> Div {
    let header = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color::foreground())
                .child(title.to_owned()),
        )
        .children(description.map(|d| {
            div()
                .text_sm()
                .text_color(color::muted_foreground())
                .child(d.to_owned())
        }));
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_5()
        .rounded_xl()
        .bg(color::card())
        .border_1()
        .border_color(color::border())
        .shadow_sm()
        .child(header)
}

/// Status tag: an adjective in sentence case, never clickable. Colour is never the only signal.
pub fn badge(label: impl Into<SharedString>, tone: Tone) -> Div {
    let c = tone.color();
    div()
        .flex_none()
        .px_2()
        .py_0p5()
        .rounded(px(4.))
        .bg(tint(c, 0.14))
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(c)
        .child(label.into())
}

/// Visible keyboard focus and Tab order for anything operable.
pub fn focusable(element: Stateful<Div>) -> Stateful<Div> {
    element.tab_stop(true).focus(|s| s.border_2().border_color(color::foreground()))
}

/// The one filled action on a page.
pub fn primary(id: impl Into<ElementId>, text: impl Into<SharedString>, enabled: bool) -> Stateful<Div> {
    focusable(
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h_11()
            .px_5()
            .rounded_lg()
            .border_1()
            .border_color(color::foreground())
            .bg(color::foreground())
            .text_color(color::background())
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(rgb(0xd9d9d9))))
            .when(!enabled, |d| d.opacity(0.45))
            .child(text.into()),
    )
}

/// Quiet underlined action, 44 px tall so it is easy to hit.
pub fn action(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    focusable(
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .h_11()
            .px_2()
            .rounded_lg()
            .border_1()
            .border_color(rgba(0))
            .text_sm()
            .underline()
            .text_color(color::foreground())
            .cursor_pointer()
            .hover(|s| s.bg(color::muted()))
            .child(text.into()),
    )
}

/// Page title and one line saying what the page is for.
pub fn page_header(title: &str, lead: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_xl().font_weight(FontWeight::MEDIUM).text_color(color::foreground()).child(title.to_owned()))
        .child(div().text_sm().text_color(color::muted_foreground()).child(lead.to_owned()))
}

/// What went wrong and how to fix it, at the top of the page.
pub fn error_summary(message: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_4()
        .rounded_lg()
        .border_2()
        .border_color(color::danger())
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(color::foreground()).child("There is a problem"))
        .child(div().text_sm().text_color(color::danger()).child(message.into()))
}

/// One thing's facts: heading, optional detail and status tag, then `summary_row`s.
pub fn summary_card(title: impl Into<SharedString>, detail: impl Into<SharedString>, status: Option<Div>) -> Div {
    div()
        .flex()
        .flex_col()
        .p_5()
        .rounded_xl()
        .bg(color::card())
        .border_1()
        .border_color(color::border())
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .pb_3()
                .child(div().font_weight(FontWeight::MEDIUM).text_color(color::foreground()).child(title.into()))
                .child(div().text_sm().text_color(color::muted_foreground()).child(detail.into()))
                .child(div().flex_1())
                .children(status),
        )
}

/// Key, value and at most one action that changes that value.
pub fn summary_row(key: &str, value: impl IntoElement, action: Option<AnyElement>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_4()
        .min_h_11()
        .border_t_1()
        .border_color(color::border())
        .child(div().w(px(150.)).flex_none().text_sm().text_color(color::muted_foreground()).child(key.to_owned()))
        .child(div().flex_1().min_w_0().overflow_hidden().text_sm().text_color(color::foreground()).child(value))
        .children(action)
}

/// Keyboard shortcut chip.
pub fn kbd(keys: &str) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .bg(color::muted())
        .border_1()
        .border_color(color::input())
        .font_family(MONO)
        .text_xs()
        .text_color(color::muted_foreground())
        .child(keys.to_owned())
}

/// Uppercase eyebrow label above a value.
pub fn label(text: &str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color::subtle_foreground())
        .child(text.to_uppercase())
}

/// Small metric tile: label, value, tone dot.
pub fn stat(title: &str, value: impl Into<SharedString>, tone: Tone) -> Div {
    div()
        .flex_1()
        .min_w(px(96.))
        .flex()
        .flex_col()
        .gap_1p5()
        .p_3()
        .rounded_lg()
        .bg(color::muted())
        .border_1()
        .border_color(color::border())
        .child(label(title))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().size_2().rounded_full().bg(tone.color()))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color::foreground())
                        .child(value.into()),
                ),
        )
}

pub fn separator() -> Div {
    div().h(px(1.)).w_full().bg(color::border())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Neutral filled.
    Secondary,
    /// Transparent with border.
    Outline,
    /// Highlighted: the control whose state is currently in effect.
    Active,
    /// Solid red; reserved for emergency stop.
    Destructive,
}

/// Full-width action row: label on the left, shortcut chip on the right.
pub fn button(
    id: &'static str,
    text: &str,
    keys: &str,
    variant: Variant,
    enabled: bool,
) -> Stateful<Div> {
    let (bg, hover, border, fg) = match variant {
        Variant::Secondary => (
            color::muted(),
            color::accent(),
            color::border(),
            color::foreground(),
        ),
        Variant::Outline => (
            tint(color::background(), 0.0),
            color::muted(),
            color::input(),
            color::foreground(),
        ),
        Variant::Active => (
            color::foreground(),
            rgb(0xd9d9d9),
            color::foreground(),
            color::background(),
        ),
        Variant::Destructive => (
            color::danger_solid(),
            color::danger_hover(),
            color::danger(),
            color::foreground(),
        ),
    };
    let chip = if variant == Variant::Destructive {
        div()
            .flex_none()
            .px_1p5()
            .py_0p5()
            .rounded_sm()
            .bg(tint(color::background(), 0.25))
            .font_family(MONO)
            .text_xs()
            .text_color(color::foreground())
            .child(keys.to_owned())
    } else {
        kbd(keys)
    };
    focusable(div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .h_11()
        .px_4()
        .rounded_lg()
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .cursor_pointer()
        .when(!enabled, |b| b.opacity(0.45))
        .hover(move |s| s.bg(hover))
        .child(text.to_owned())
        .child(chip))
}

/// The Lumen mark, compiled in so it appears even before any data is installed.
pub fn mark(size: f32) -> Img {
    static MARK: std::sync::LazyLock<std::sync::Arc<Image>> = std::sync::LazyLock::new(|| {
        std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../../../packs/brand/lumen-mark-light.png").to_vec()))
    });
    img(MARK.clone()).size(px(size)).flex_none()
}
