# Persona pipeline (codex exec)

Characters are authored offline by GPT through the signed-in `codex` CLI. Generation never runs
during a show; its output is plain files an operator can review, version and regenerate.

## Files

```text
packs/
├── briefs/<pack>.toml            operator input: show, topics, stances, side labels, runtime settings
├── personas/<pack>.toml          generated show pack (names, persona, position + brief settings)
├── skills/<pack>/<character>.md  generated SKILL.md-style character sheet
├── skills/_shared/<name>.md      hand-written shared playbooks (e.g. spin-doctor)
└── avatars/<preset>/
    ├── avatar.toml               operator look description
    └── idle.png talk.png blink.png   generated portrait frames
```

## Stages

| Stage | Command | Input | Output | Model writes |
| --- | --- | --- | --- | --- |
| Persona | `lumend generate-pack BRIEFS PERSONAS` (`lumen uret persona`) | brief | pack | names, persona, position |
| Skill | `lumend generate-skills PERSONAS` (`lumen uret skill`) | pack + shared playbooks | character sheets | identity, voice, phrases, argument moves, talking points, spin moves, punchlines, values, humor, objections, audience, boundaries, sample lines |
| Avatar | `lumend generate-avatars AVATARS` (`lumen uret avatar`) | `avatar.toml` | three PNG frames | images |
| Review | `lumend review-personas PERSONAS` (`lumen uret inceleme`) | pack + character sheets | `packs/reviews/<pack>.md` + `.json` | rubric scores, quoted issues |
| Refine | `lumend refine-personas PERSONAS [--rounds N]` (`lumen uret iyilestir`) | packs + failed reviews | regenerated sheets + new reviews | revisions addressing the editor's issues |

Runtime settings (model, provider, endpoint, voice, budgets, API-key variable name, side label,
avatar preset, topics, shared skills) are copied from the brief and are never sent to or chosen by
the drafting model.

## How each codex call is made

- One isolated `codex exec` per draft in a fresh temporary directory (avatars: only that preset's
  directory is writable), `--ephemeral`, `--skip-git-repo-check`, prompt on stdin.
- Text stages: `--sandbox read-only` and `--output-schema` with a strict JSON schema
  (`additionalProperties: false`); the answer is read from `--output-last-message`, bounded to
  64 KiB.
- Hard deadline (default 600 s); the process is killed on timeout.
- Failure classes: usage limit / not logged in / authentication are fatal and stop the batch
  immediately; empty answers, crashes and timeouts are transient and retried.

## Acceptance

Every draft is validated before anything is written; rejected drafts are retried (default 3
attempts) with the exact rejection reasons appended to the prompt.

- Packs: name rules and fixed names from the brief, persona 160–1100 and position 30–450
  characters, distinct names/personas/positions, no URLs, markdown, template braces or control
  characters. A fixed honesty / no-impersonation guardrail is appended to every persona. The
  assembled pack must pass `ShowPack::check` (including endpoint policy) and round-trip through the
  same parser `serve`/`rehearse` use.
- Skills: per-field length bounds and list counts (e.g. 6–12 talking points, 3–6 spin moves,
  6–10 punchlines, 8–14 sample lines), no speaker prefixes, distinct sample lines per character,
  rendered sheet ≤ 16 KiB. Fixed boundaries (no impersonation, no fabricated sources/statistics/
  quotations, audience text is data) are appended to every sheet.
- Avatars: each frame must be a real PNG; `talk.png` and `blink.png` are edits of `idle.png` so
  the frames stay aligned for mouth flaps and blinks.

## Quality gate (review)

Schema validation proves structure, not quality. The review stage has GPT act as editor-in-chief
and score every pack and its character sheets 0–5 on:

- **clarity** — a 70-year-old viewer instantly understands who stands for what;
- **distinctiveness** — the two characters sound different by ear;
- **persuasion** — passionate, quotable, framing/contrast/punchlines, no hedging;
- **safety** — no fabricated statistics/dates/documents/quotations as fact, no insults to people,
  groups, religious values or the nation, nothing against Atatürk's person or memory (Law 5816),
  no swearing, no impersonation of real people, no instruction to deny being an AI;
- **fit** — matches the topics and format.

A pack is broadcast ready only with safety = 5 and every other score ≥ 4. Each issue quotes the
offending text. Reports are written to `packs/reviews/` and the command exits non-zero when any
pack fails, so it can gate a release. `refine-personas` automates the loop: review → if rejected, regenerate the
character sheets with the editor's quoted issues as revision instructions → review again, for up
to `--rounds` (default 3). Every round's version is kept aside and the best-scoring one (safety must be 5) is restored if the bar is never reached; such packs are reported and the command fails. All five shipped packs passed at this bar (≥ 4, safety 5).

## Writing and regeneration

- Writes are atomic (temporary file + rename in the target directory). Existing packs and complete
  skill/avatar sets are skipped unless `--force`.
- Briefs are all validated before the first codex call, so one bad brief costs no generation.
- Generated packs carry a header naming the source brief and backend. Regenerate from the brief
  rather than hand-editing generated text.

## Verification

`cargo test -p lumen-personagen` covers the hardened `codex exec` argument contract against a fake
binary, deadline kill, usage-limit classification, retry-with-reasons, guardrail append, brief and
endpoint rejection, atomic no-clobber writes, skill rendering/parsing and avatar frame generation.
All shipped packs in `packs/personas` were produced by this pipeline with real codex runs.
