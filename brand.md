# Lumen brand

Lumen is an operator's tool for running live shows. It should feel calm, plain and exact while something stressful is happening. The mark and the monochrome palette do not change. How the interface behaves follows the GOV.UK Design System's principles.

## Mark

| File | Use |
| --- | --- |
| `packs/brand/lumen-mark.svg` | Dark mark on light grounds |
| `packs/brand/lumen-mark-light.svg` | Light mark on dark grounds (the Studio) |
| `packs/brand/org.lumen.Lumen.svg` | App icon on the desktop |

- The mark is one colour: `#0d0d0d` on light, `#fafafa` on dark.
- Never recolour, outline, rotate or add effects to it, and never rebuild it from another shape.
- Keep clear space of at least a quarter of its height on every side. Minimum size is 16 px.
- The name is always written **Lumen**. The mark is not used as a status indicator or a button.

## Colour

The interface is monochrome. Colour appears only to report state, and never as the only signal: a state always has words.

| Token (`ui::color`) | Value | Contrast on black | Use |
| --- | --- | --- | --- |
| `background` | `#000000` | | Page |
| `card` | `#0d0d0d` | | Cards |
| `muted` | `#222222` | | Secondary fills, hover |
| `accent` | `#2e2e2e` | | Hover on secondary fills |
| `border` | `#404040` | 2.0:1 | Decorative dividers only |
| `input` | `#8c8c8c` | 6.3:1 | Edge of anything you can operate |
| `foreground` | `#fafafa` | 20.1:1 | Text, primary action fill, focus |
| `muted_foreground` | `#b3b3b3` | 10.0:1 | Secondary text |
| `subtle_foreground` | `#8c8c8c` | 6.3:1 | Keys and eyebrow labels |
| `success` | `#22c55e` | 9.2:1 | Working as intended |
| `warning` | `#f59e0b` | 9.8:1 | Needs attention soon |
| `danger` | `#ef4444` | 5.6:1 | Failed, or destroys something |
| `info` | `#60a5fa` | 8.3:1 | Neutral fact worth noticing |

`danger_solid` (`#dc2626`) is reserved for the emergency stop. The broadcast frame has its own editable look in `theme.toml`, which starts from the same palette.

## Type

- Interface: Nimbus Sans (a Helvetica-style face). Code, keys and identifiers: JetBrainsMono Nerd Font.
- One page title (`text_xl`), then card titles (`text_sm` semibold), then body (`text_sm`). Text below `text_sm` is only for keys and eyebrow labels.
- Normal or medium weight for headings. Sentence case everywhere, except eyebrow labels.

## Shape and space

- 4 px spacing grid. Cards have a 12 px radius, controls 8 px, tags 4 px.
- Anything you can operate is at least 44 px tall.
- No gradients, glows or decorative icons.

## Principles from the GOV.UK Design System

Adopted from [alphagov/govuk-design-system](https://github.com/alphagov/govuk-design-system) (MIT, © Crown Copyright). We use its reasoning and patterns, rebuilt natively in GPUI.

**What we do not take.** Lumen is not a government service and must never look like one. We do not use the GOV.UK name or logo, the Royal Crown, the GDS Transport typeface, or the blue GOV.UK header and footer. GOV.UK's yellow focus colour is replaced by our monochrome focus ring.

| Rule | In Lumen |
| --- | --- |
| Start with what the user needs to do | Pages are named after jobs. Remove anything that does not help the operator act |
| One primary action per page | One filled button (`ui::primary`). Everything else is a quiet action |
| Say what things do, plainly | Buttons start with a verb ("Sign in", "Add connection"). Short sentences, no jargon or marketing |
| Show status with tags | `ui::badge`: adjectives only ("Live", "Failed"), never clickable, sentence case |
| Summarise facts as key and value | `ui::summary_row`: key, value, and one action that names what it changes |
| Group a thing's facts in a summary card | `ui::summary_card` for each connection |
| Tell people what went wrong and how to fix it | `ui::error_summary` at the top of the page. "Enter a username", not "Invalid input" |
| Ask only what you need | Adding a connection asks for a platform and a username. Everything else has a default |
| Make the risky step explicit | Destructive or authorising actions take a second, clearly worded click |
| Focus must always be visible | Thick `foreground` edge on every operable element (`ui::focusable`), reachable with Tab |
| Do not rely on colour alone | Every coloured state also has words |
| Controls must be distinguishable | Non-text contrast of at least 3:1 for control edges, so they use `input`, not `border` |

## Product rules

- English is the default language of the app, its on-screen text and the docs.
- Lumen ships with no named show of its own. The user names every show, and the first-run guide asks for it.
- **Ctrl K** (**⌘ K** on Mac) is the one shortcut advertised for moving around. It opens Find. Navigation never prints shortcut hints.
- The Lumen mark appears in the sidebar, the first-run guide and Info, compiled into the app so it never goes missing.

## Writing

- Address the operator as "you". Say what happened, then what to do next.
- Name real things: "Sign in on the platform's own page", not "Authenticate".
- Never claim something works if it has not been checked against the real platform.

## Where this lives

`apps/lumen-studio/src/ui.rs` is the only place interface colours and components are defined. A new pattern is added there and described here.
