# Regenerates the README cards and platform icons in both system themes:
#   python3 assets/readme/generate.py assets/readme
#
# Follows brand.md: monochrome tokens, Helvetica-style type, normal-weight headings,
# 12 px cards, no glow or accent colour. Platform and system marks are single-colour
# (white on dark, near-black on light). They identify the services Lumen works with.
import re
import sys
from pathlib import Path

out = Path(sys.argv[1])
root = Path(__file__).resolve().parents[2]
source = (root / "packs/brand/lumen-mark.svg").read_text()
MARK_PATH = re.search(r' d="([^"]+)"', source).group(1)

THEMES = {
    "dark": dict(background="#000000", text="#ffffff", muted="#b3b3b3", border="#404040", mark="#ffffff"),
    "light": dict(background="#ffffff", text="#0d0d0d", muted="#333333", border="#b3b3b3", mark="#0d0d0d"),
}
SANS = 'font-family="&quot;Helvetica Neue&quot;, Helvetica, Arial, sans-serif"'
MONO = 'font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"'
WIDTH = 960

TIKTOK = "M12.525.02c1.31-.02 2.61-.01 3.91-.02.08 1.53.63 3.09 1.75 4.17 1.12 1.11 2.7 1.62 4.24 1.79v4.03c-1.44-.05-2.89-.35-4.2-.97-.57-.26-1.1-.59-1.62-.93-.01 2.92.01 5.84-.02 8.75-.08 1.4-.54 2.79-1.35 3.94-1.31 1.92-3.58 3.17-5.91 3.21-1.43.08-2.86-.31-4.08-1.03-2.02-1.19-3.44-3.37-3.65-5.71-.02-.5-.03-1-.01-1.49.18-1.9 1.12-3.72 2.58-4.96 1.66-1.44 3.98-2.13 6.15-1.72.02 1.48-.04 2.96-.04 4.44-.99-.32-2.15-.23-3.02.37-.63.41-1.11 1.04-1.36 1.75-.21.51-.15 1.07-.14 1.61.24 1.64 1.82 3.02 3.5 2.87 1.12-.01 2.19-.66 2.77-1.61.19-.33.4-.67.41-1.06.1-1.79.06-3.57.07-5.36.01-4.03-.01-8.05.02-12.07z"
OS_MARKS = {'apple': ('0 0 24 24', ['M16.4 12.6c0-2.4 2-3.6 2.1-3.7-1.2-1.7-3-1.9-3.6-2-1.5-.2-3 .9-3.8.9-.8 0-2-.9-3.3-.9-1.7 0-3.3 1-4.2 2.5-1.8 3.1-.5 7.7 1.3 10.2.8 1.2 1.8 2.6 3.1 2.6 1.2-.1 1.7-.8 3.2-.8s1.9.8 3.2.8c1.3 0 2.2-1.3 3-2.5 1-1.4 1.3-2.7 1.4-2.8-.1 0-2.4-.9-2.4-4.3zM14 5.4c.7-.8 1.1-1.9 1-3-1 0-2.1.7-2.8 1.5-.6.7-1.2 1.8-1 2.9 1.1.1 2.1-.6 2.8-1.4z'], False), 'linux': ('0 0 24 24', ['M12 1.5c-2.6 0-4 2.2-4 5 0 1.4.2 2.5-.7 3.9C5.8 12.6 4.5 14.8 4.5 17c0 .9.2 1.6.5 2.2-.9.4-1.5.9-1.5 1.5 0 1 2.2 1.8 4.4 1.8 1.4 0 2.6-.4 3.1-.9h2c.5.5 1.7.9 3.1.9 2.2 0 4.4-.8 4.4-1.8 0-.6-.6-1.1-1.5-1.5.3-.6.5-1.3.5-2.2 0-2.2-1.3-4.4-2.8-6.6-.9-1.4-.7-2.5-.7-3.9 0-2.8-1.4-5-4-5zM10.4 5.2a1 1 0 1 0 0 2 1 1 0 0 0 0-2zm3.2 0a1 1 0 1 0 0 2 1 1 0 0 0 0-2zM12 11c-2 0-3.5 2.4-3.5 5.2 0 1.7.6 2.8 1.5 3.3h4c.9-.5 1.5-1.6 1.5-3.3 0-2.8-1.5-5.2-3.5-5.2z', 'M10.6 8.2h2.8L12 9.6z'], True), 'windows': ('0 0 24 24', ['M2.5 4.7 10.3 3.6v7.7H2.5zM11.3 3.5 21.5 2v9.3H11.3zM2.5 12.3h7.8V20l-7.8-1.1zM11.3 12.3h10.2V22l-10.2-1.5z'], False)}


def brand(name, colour):
    """A platform mark in a 24 x 24 grid, one colour only."""
    if name == "tiktok":
        return f'<path fill="{colour}" d="{TIKTOK}"/>'
    if name == "kwai":
        return (
            f'<g fill="none" stroke="{colour}" stroke-width="1.8" stroke-linejoin="round">'
            '<circle cx="9.3" cy="6.6" r="3.9"/><circle cx="17.6" cy="7.2" r="2.8"/>'
            '<rect x="9.3" y="13.3" width="11.2" height="8.3" rx="2.6"/>'
            '<path d="M3.2 14.6 L7.1 17.4 L3.2 20.2 Z"/></g>'
        )
    if name == "likee":
        heart = "M12 21.6 C11.4 21.6 10.8 21.3 10.2 20.8 L3.4 14.3 C1.4 12.4 .8 9.6 1.7 7.2 C2.7 4.6 5.1 3 7.7 3 C9.6 3 11 3.7 12.3 5 C13.6 3.7 15.3 3 17 3 C19.7 3.1 22 4.8 22.9 7.3 C23.8 9.9 23.1 12.5 21.1 14.6 L14.2 20.8 C13.4 21.4 12.7 21.6 12 21.6 Z"
        return (
            '<mask id="likee-gaps"><rect width="24" height="24" fill="#fff"/>'
            '<path d="M3.5 15 L15.5 3 M8.3 20.3 L21.8 6.6" stroke="#000" stroke-width="1"/></mask>'
            f'<path fill="{colour}" mask="url(#likee-gaps)" d="{heart}"/>'
        )
    raise KeyError(name)


# Line pictograms in a 32 x 32 grid.
ICONS = {
    "name": '<path d="M5 8 H27 M16 8 V26 M11 26 H21"/>',
    "ask": '<path d="M4 6 H28 V22 H14 L8 28 V22 H4 Z"/><path d="M12 12 H20 M12 16 H17"/>',
    "rehearse": '<rect x="3" y="5" width="26" height="18" rx="4"/><path d="M13 10 L19 14 L13 18 Z"/><path d="M3 28 H29"/>',
    "live": '<circle cx="16" cy="16" r="4"/><path d="M9 9 C5 13 5 19 9 23 M23 9 C27 13 27 19 23 23 M5 5 C-1 11 -1 21 5 27 M27 5 C33 11 33 21 27 27"/>',
    "accounts": '<circle cx="11" cy="11" r="5"/><path d="M3 27 C3 21 7 18 11 18 C15 18 19 21 19 27"/><path d="M21 7 C24 7 26 9 26 12 C26 15 24 17 21 17 M23 19 C27 20 29 23 29 27"/>',
    "chat": '<path d="M3 5 H21 V17 H11 L6 22 V17 H3 Z"/><path d="M25 11 H29 V23 H26 V28 L21 23 H13"/>',
    "cast": '<rect x="11" y="3" width="10" height="7" rx="2"/><path d="M16 10 V16 M6 22 V16 H26 V22"/><rect x="2" y="22" width="8" height="7" rx="2"/><rect x="22" y="22" width="8" height="7" rx="2"/>',
    "look": '<circle cx="16" cy="16" r="13"/><circle cx="10" cy="13" r="1.4"/><circle cx="16" cy="9" r="1.4"/><circle cx="22" cy="13" r="1.4"/><path d="M16 29 C12 25 18 21 22 22"/>',
    "control": '<path d="M5 9 H27 M5 16 H27 M5 23 H27"/><circle cx="11" cy="9" r="2.6"/><circle cx="21" cy="16" r="2.6"/><circle cx="13" cy="23" r="2.6"/>',
    "lock": '<rect x="6" y="14" width="20" height="14" rx="3"/><path d="M10 14 V10 C10 6 13 3.5 16 3.5 C19 3.5 22 6 22 10 V14"/>',
}


def esc(text):
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


class Canvas:
    def __init__(self, theme):
        self.t = THEMES[theme]
        self.parts = []

    def card(self, x, y, w, h):
        self.parts.append(f'<rect x="{x + 0.5}" y="{y + 0.5}" width="{w - 1}" height="{h - 1}" rx="12" fill="{self.t["background"]}" stroke="{self.t["border"]}"/>')

    def text(self, x, y, content, size=16, weight=400, muted=False, mono=False, anchor="start"):
        colour = self.t["muted"] if muted else self.t["text"]
        self.parts.append(f'<text x="{x}" y="{y}" font-size="{size}" font-weight="{weight}" fill="{colour}" text-anchor="{anchor}" {MONO if mono else SANS}>{esc(content)}</text>')

    def pill(self, right, y, label):
        width = 20 + len(label) * 7.2
        self.parts.append(f'<rect x="{right - width + 0.5}" y="{y + 0.5}" width="{width - 1}" height="23" rx="4" fill="none" stroke="{self.t["border"]}"/>')
        self.text(right - width / 2, y + 16, label, size=13, muted=True, anchor="middle")

    def icon(self, x, y, name, size=32):
        self.parts.append(f'<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 32 32" aria-hidden="true"><g fill="none" stroke="{self.t["mark"]}" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">{ICONS[name]}</g></svg>')

    def brand(self, x, y, name, size=40):
        self.parts.append(f'<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 24 24" aria-hidden="true">{brand(name, self.t["mark"])}</svg>')

    def mark(self, x, y, size):
        self.parts.append(f'<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 100 100" aria-hidden="true"><g transform="translate(0 1.1)" fill="{self.t["mark"]}"><path fill-rule="evenodd" transform="scale(0.091575)" d="{MARK_PATH}"/></g></svg>')

    def save(self, name, theme, height, label):
        (out / f"{name}-{theme}.svg").write_text(
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" viewBox="0 0 {WIDTH} {height}" role="img" aria-label="{esc(label)}">\n' + "\n".join(self.parts) + "\n</svg>\n")


def wrap(c, x, y, lines, size=14, gap=22):
    for i, line in enumerate(lines):
        c.text(x, y + i * gap, line, size=size, muted=True)


for theme in THEMES:
    c = Canvas(theme)
    c.card(0, 0, WIDTH, 260)
    mark, gap, name_width = 48, 14, 92
    left = (WIDTH - (mark + gap + name_width)) / 2
    c.mark(left, 40, mark)
    c.text(left + mark + gap, 76, "Lumen", size=32)
    c.text(WIDTH / 2, 160, "Two AI hosts debate live. Your viewers join in.", size=34, anchor="middle")
    c.text(WIDTH / 2, 206, "Name your show, ask a question, and stream it to your accounts.", size=16, muted=True, anchor="middle")
    c.save("hero", theme, 260, "Lumen. Two AI hosts debate live. Your viewers join in.")

    c = Canvas(theme)
    c.text(0, 26, "Works with", size=18)
    item_w, item_h, g = (WIDTH - 2 * 24) / 3, 172, 24
    platforms = [
        ("tiktok", "TikTok", ["Reads your LIVE chat. Streams", "with a key from LIVE Producer."], "Chat tested"),
        ("kwai", "Kwai", ["Reads your LIVE chat. Creates and", "starts rooms through Kwai Studio."], "Chat tested"),
        ("likee", "Likee", ["Profiles and comments work.", "LIVE chat is still being tested."], "Early"),
    ]
    for i, (key, title, lines, status) in enumerate(platforms):
        x = i * (item_w + g)
        c.card(x, 48, item_w, item_h)
        c.brand(x + 20, 68, key)
        c.pill(x + item_w - 20, 76, status)
        c.text(x + 20, 148, title, size=18, weight=500)
        wrap(c, x + 20, 176, lines)
    c.save("platforms", theme, 48 + item_h + 2, "Works with TikTok, Kwai and Likee")

    c = Canvas(theme)
    c.card(0, 0, WIDTH, 236)
    c.text(32, 52, "How it works", size=18)
    steps = [
        ("name", "Name your show", ["Call it anything. Change", "it whenever you like."]),
        ("ask", "Ask a question", ["Your two hosts take", "opposite sides."]),
        ("rehearse", "Rehearse", ["Watch it privately. Nothing", "goes live until you say so."]),
        ("live", "Go live", ["Hosts debate on screen", "and answer your viewers."]),
    ]
    column = (WIDTH - 64 - 3 * 32) / 4
    for i, (icon, title, lines) in enumerate(steps):
        x = 32 + i * (column + 32)
        c.icon(x, 84, icon)
        c.text(x + column - 8, 104, f"0{i + 1}", size=14, muted=True, mono=True, anchor="end")
        c.text(x, 152, title, size=16, weight=500)
        wrap(c, x, 180, lines)
    c.save("steps", theme, 236, "How it works: name your show, ask a question, rehearse, go live")

    c = Canvas(theme)
    features = [
        ("accounts", "Many accounts at once", ["Connect as many TikTok, Kwai and", "Likee accounts as you want."]),
        ("chat", "One chat for everything", ["Comments arrive in one place, with", "spam and abuse filtered out."]),
        ("cast", "Stream to several places", ["One show to all your accounts.", "If one fails, the others carry on."]),
        ("look", "Make it yours", ["Change colours, fonts and", "backgrounds, and see it instantly."]),
        ("control", "Stay in control", ["Skip a question, pass the floor,", "or stop everything with one button."]),
        ("lock", "Your password stays yours", ["You sign in on each platform's own", "page. Sign-ins are stored encrypted."]),
    ]
    c.text(0, 26, "What you can do", size=18)
    item_w, item_h, g = (WIDTH - 2 * 24) / 3, 152, 24
    for i, (icon, title, lines) in enumerate(features):
        x, y = (i % 3) * (item_w + g), 48 + (i // 3) * (item_h + g)
        c.card(x, y, item_w, item_h)
        c.icon(x + 20, y + 20, icon)
        c.text(x + 20, y + 84, title, size=16, weight=500)
        wrap(c, x + 20, y + 110, lines)
    c.text(0, 48 + 2 * item_h + g + 40, "Press Ctrl K (⌘ K on Mac) to find anything.", size=14, muted=True)
    c.save("features", theme, 48 + 2 * item_h + g + 56, "What you can do")

    for name, (box, paths, odd) in OS_MARKS.items():
        rule = ' fill-rule="evenodd"' if odd else ""
        body = "".join(f'<path fill="{THEMES[theme]["mark"]}"{rule} d="{d}"/>' for d in paths)
        (out / f"os-{name}-{theme}.svg").write_text(f'<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="{box}" role="img" aria-label="{name.title()}">{body}</svg>\n')
    for name in ("tiktok", "kwai", "likee"):
        (out / f"platform-{name}-{theme}.svg").write_text(f'<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" role="img" aria-label="{name.title()}">{brand(name, THEMES[theme]["mark"])}</svg>\n')
