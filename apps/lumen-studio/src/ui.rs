//! Studio design system, following the onur.af design language: monochrome tokens (black
//! ground, white text, #b3b3b3 muted, #404040 borders, #222 surfaces), Helvetica-style type,
//! 12 px cards, 8 px controls at least 44 px tall, filled primary action, 4 px spacing grid.
//!
//! Rules: one neutral scale for surfaces/borders/text, semantic colors only for state
//! (success/warning/danger/info), 4px spacing grid, radius 6 (controls) / 12 (cards),
//! one accent per element. Components here are the only place colors are chosen.
use gpui::{prelude::*, *};

pub const FONT: &str = "Nimbus Sans";
pub const MONO: &str = "JetBrainsMono Nerd Font";

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
    pub fn input() -> Rgba {
        rgb(0x404040)
    }
    pub fn foreground() -> Rgba {
        rgb(0xfafafa)
    }
    pub fn muted_foreground() -> Rgba {
        rgb(0xb3b3b3)
    }
    pub fn subtle_foreground() -> Rgba {
        rgb(0x808080)
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
                .text_xs()
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

/// Pill with a status dot, e.g. "● Connected".
pub fn badge(label: impl Into<SharedString>, tone: Tone) -> Div {
    let c = tone.color();
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .px_2p5()
        .py_0p5()
        .rounded_full()
        .bg(tint(c, 0.12))
        .border_1()
        .border_color(tint(c, 0.28))
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(c)
        .child(div().size_1p5().rounded_full().bg(c))
        .child(label.into())
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
    div()
        .id(id)
        .tab_stop(true)
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
        .focus(|s| s.border_color(color::foreground()))
        .child(text.to_owned())
        .child(chip)
}

/// The Lumen mark (light version for the dark UI), from `packs/brand`.
pub fn mark(size: f32) -> Img {
    img(crate::theme::lumen_path("brand/lumen-mark-light.png"))
        .size(px(size))
        .flex_none()
}
