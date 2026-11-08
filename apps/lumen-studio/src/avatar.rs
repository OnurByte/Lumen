//! VTuber-style 2D hosts drawn from primitives: idle breathing and head bob, periodic blinks,
//! and a mouth that opens with the live speech level. Presets are picked per agent by the pack
//! (`avatar = "fesli" | "ogretmen"`); anything else gets a neutral look.
use gpui::{prelude::*, *};

/// Canvas size in frame design units (the caller's `u` scales it).
pub const W: f32 = 170.;
pub const H: f32 = 176.;

struct Look {
    skin: u32,
    hair: u32,
    coat: u32,
    collar: u32,
}

fn rgba(c: u32, a: f32) -> Rgba {
    Rgba { a, ..rgb(c) }
}

/// Draws one host. `t` is seconds since the room opened; `level` is 0..1 speech energy.
pub fn draw(
    preset: &str,
    index: usize,
    level: f32,
    speaking: bool,
    t: f32,
    u: &dyn Fn(f32) -> Pixels,
) -> Div {
    let look = match preset {
        "fesli" => Look {
            skin: 0xd9a27a,
            hair: 0xd8d6d0,
            coat: 0x1f2a44,
            collar: 0xf4f1ea,
        },
        "ogretmen" => Look {
            skin: 0xf0c4a0,
            hair: 0x6f6259,
            coat: 0x7a1f2b,
            collar: 0xf7efe6,
        },
        _ => Look {
            skin: [0xe8b48a, 0xc98e63][index % 2],
            hair: [0x3a2416, 0x1f1712][index % 2],
            coat: [0x22b8a6, 0xf28a3c][index % 2],
            collar: 0xf4f1ea,
        },
    };
    let phase = index as f32 * 1.7;
    // Idle motion: slow breathing, a little more energetic while speaking.
    let bob = (t * 1.6 + phase).sin() * if speaking { 2.6 } else { 1.4 } + level * 2.0;
    let sway = (t * 0.9 + phase).sin() * 1.2;
    let blink = (t + phase * 0.8) % 4.3 < 0.13;
    let open = level > 0.06;

    let at = |left: f32, top: f32, w: f32, h: f32| {
        div().absolute().left(u(left)).top(u(top)).w(u(w)).h(u(h))
    };

    let eye = |left: f32| {
        if blink {
            return at(left, 56., 18., 3.).rounded_full().bg(rgb(0x2a1d16));
        }
        at(left, 47., 18., 21.)
            .rounded_full()
            .bg(rgb(0xffffff))
            .child(
                at(3.5, 4., 11., 14.)
                    .rounded_full()
                    .bg(rgb(if preset == "fesli" {
                        0x4a3426
                    } else {
                        0x2f4a6b
                    }))
                    .child(at(2., 2., 4., 4.).rounded_full().bg(rgb(0xffffff))),
            )
    };
    let brow = |left: f32, tilt: f32| {
        at(left, 38. - tilt, 20., 4.)
            .rounded_full()
            .bg(rgb(if preset == "fesli" {
                0xcfcac2
            } else {
                look.hair
            }))
    };
    let mouth = if open {
        let w = 18. + 8. * level;
        let h = 5. + 15. * level;
        at(50. - w / 2., 82., w, h)
            .rounded_full()
            .bg(rgb(0x5a1a1f))
            .child(
                at(w * 0.25, h * 0.55, w * 0.5, h * 0.4)
                    .rounded_full()
                    .bg(rgb(0xd96a6a)),
            )
    } else {
        at(39., 86., 22., 4.).rounded_full().bg(rgb(0x8a3b36))
    };

    let mut head = at(35. + sway, 10. + bob, 100., 112.)
        .rounded_full()
        .bg(rgb(look.skin))
        // Blush.
        .child(
            at(10., 70., 18., 9.)
                .rounded_full()
                .bg(rgba(0xff7a8a, 0.35)),
        )
        .child(
            at(72., 70., 18., 9.)
                .rounded_full()
                .bg(rgba(0xff7a8a, 0.35)),
        )
        .child(eye(23.))
        .child(eye(59.))
        .child(brow(22., if speaking { 1.5 } else { 0. }))
        .child(brow(58., if speaking { 1.5 } else { 0. }));

    match preset {
        "fesli" => {
            head = head
                // Beard, then moustache, then mouth on top.
                .child(at(10., 70., 80., 50.).rounded_b(u(40.)).bg(rgb(look.hair)))
                .child(at(14., 64., 72., 16.).rounded_full().bg(rgb(look.hair)))
                .child(mouth)
                .child(at(31., 76., 38., 8.).rounded_full().bg(rgb(0xe9e7e2)));
        }
        "ogretmen" => {
            let lens = |left: f32| {
                at(left, 44., 28., 25.)
                    .rounded_full()
                    .border(u(2.5))
                    .border_color(rgb(0x2b2b33))
                    .bg(rgba(0xffffff, 0.08))
            };
            head = head
                .child(mouth)
                .child(lens(18.))
                .child(lens(54.))
                .child(at(46., 54., 8., 2.5).bg(rgb(0x2b2b33)))
                .child(at(-3., 84., 6., 6.).rounded_full().bg(rgb(0xe0b440)))
                .child(at(97., 84., 6., 6.).rounded_full().bg(rgb(0xe0b440)));
        }
        _ => head = head.child(mouth),
    }

    let mut canvas = div()
        .relative()
        .w(u(W))
        .h(u(H))
        // Shoulders / coat with a collar.
        .child(
            at(18., 124. + bob * 0.4, 134., 56.)
                .rounded_t(u(64.))
                .bg(rgb(look.coat)),
        )
        .child(at(68., 118. + bob * 0.6, 34., 24.).bg(rgb(look.skin)))
        .child(
            at(70., 130. + bob * 0.4, 30., 18.)
                .rounded_b(u(12.))
                .bg(rgb(look.collar)),
        );

    // Hair behind the head.
    canvas = match preset {
        "ogretmen" => canvas.child(
            at(26. + sway, 12. + bob, 118., 98.)
                .rounded(u(50.))
                .bg(rgb(look.hair)),
        ),
        "fesli" => canvas,
        _ => canvas.child(
            at(31. + sway, 6. + bob, 108., 46.)
                .rounded_t(u(54.))
                .bg(rgb(look.hair)),
        ),
    };
    canvas = canvas.child(head);
    // Hair / hat in front.
    canvas = match preset {
        "fesli" => canvas
            // Fez: tall red cap, darker band, black tassel swinging a little.
            .child(
                at(56. + sway, -6. + bob, 58., 34.)
                    .rounded_t(u(8.))
                    .bg(rgb(0xb3202a)),
            )
            .child(
                at(53. + sway, 22. + bob, 64., 8.)
                    .rounded(u(3.))
                    .bg(rgb(0x8c1720)),
            )
            .child(at(84. + sway + (t * 2.2).sin() * 2., -6. + bob, 3., 26.).bg(rgb(0x1a1a1a)))
            .child(
                at(81. + sway + (t * 2.2).sin() * 3., 18. + bob, 9., 9.)
                    .rounded_full()
                    .bg(rgb(0x1a1a1a)),
            )
            .child(
                at(38. + sway, 40. + bob, 8., 26.)
                    .rounded(u(4.))
                    .bg(rgb(look.hair)),
            )
            .child(
                at(124. + sway, 40. + bob, 8., 26.)
                    .rounded(u(4.))
                    .bg(rgb(look.hair)),
            ),
        "ogretmen" => canvas
            .child(
                at(33. + sway, 8. + bob, 104., 34.)
                    .rounded_t(u(52.))
                    .bg(rgb(look.hair)),
            )
            // Small red pin on the lapel.
            .child(
                at(40., 146. + bob * 0.4, 8., 8.)
                    .rounded_full()
                    .bg(rgb(0xd4202c)),
            ),
        _ => canvas,
    };
    canvas
}
