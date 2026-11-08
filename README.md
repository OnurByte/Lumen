# Lumen

Local-first Linux studio for supervised live debates between independent AI characters.

This repository is under construction. The complete product specification is in [LUMEN_PLAN.md](LUMEN_PLAN.md); measured progress and unmet gates are in [docs/feasibility.md](docs/feasibility.md).

## Multi-platform LIVE accounts

`lumen-live` manages any number of TikTok, Kwai Global and Likee Global accounts, each with isolated encrypted credentials, its own LIVE connection, reconnection and metrics. It merges their events into one moderated inbox for the AI hosts and can broadcast one scene to several authorized accounts. Verified and unverified capabilities, and the security model, are in [docs/platforms.md](docs/platforms.md).

```sh
lumen-live accounts add --platform kwai --label kw-main --handle my_kwai_id
lumen-live connect --socket "$XDG_RUNTIME_DIR/lumen/lumend.sock"
lumen-live capabilities
```

## Install (Linux only)

```sh
sh install.sh                  # from this checkout: builds release, installs for your user
LUMEN_TTS=1 sh install.sh      # also set up the Turkish TTS worker for live voice
sh install.sh --uninstall      # remove (packs kept unless LUMEN_PURGE=1)
```

Installs `lumend`, `lumen-studio` and the `lumen` launcher into `~/.local/bin`, packs/avatars/TTS worker into `~/.local/share/lumen`, and "Lumen Deneme Odası" / "Lumen Studio" app-menu entries. No sudo; missing system packages (Vulkan, xkbcommon, ALSA, URW fonts, build tools) are listed and installed only after you confirm. `packaging/linux/release-archive.sh` builds `dist/lumen-linux-<arch>.tar.gz` + `.sha256` for `LUMEN_ARCHIVE=... sh install.sh` or GitHub releases (`LUMEN_REPO=owner/repo`).

```sh
lumen                          # rehearsal room, Hakikat Arayışı, GPT via codex
lumen prova history-clash --senaryo   # any pack; --senaryo = skill-sheet lines, no model
lumen yayin                    # live: TTS + audio, clean 9:16 frame for OBS + Studio
lumen uret persona|skill|avatar [--force]
```

GPT characters use the signed-in `codex` CLI (`provider = "codex"`, `model = "gpt-5.6-luna"` in a pack; about 8 s per turn, shown on stage as "düşünüyor…"). HTTP/OpenAI-compatible endpoints remain available with `provider = "http"`.

## The Lumen app

`lumen` (or "Lumen" in the app menu) opens one window with a sidebar:

- **Prova** — the 9:16 broadcast frame next to the operator panel (start/pause, next question, hand over the floor, emergency stop, simulated viewers, typed comments).
- **Kontrol** — the control room for a running show.
- **Görünüm** — the appearance editor with a live preview.
- **Paket / Mod** — pick a pack and GPT rehearsal, scripted rehearsal or live; the app starts and stops `lumend` itself (log: `~/.local/state/lumen/lumend.log`) and reuses a runtime that was started elsewhere.
- **Yayın ekranını aç** — a separate clean frame window for OBS capture. Ctrl+1/2/3 switch pages.

## Appearance

The frame look lives in `~/.config/lumen/theme.toml` and is edited from **Görünüm**: background (solid, gradient, or an image carousel that cross-fades every N seconds with a readability veil; add files or a whole folder), side colours, question-card colour, font, topic/caption/name sizes and corner radii. Defaults follow the onur.af design language: monochrome (black ground, white text, #b3b3b3 muted, #404040 borders, #222 surfaces), Helvetica-style type with normal-weight headings, 12 px cards and 8 px controls, no decorative accents or glows. "Varsayılana dön" restores it. Two GPT-drawn dark studio backdrops ship in `packs/backgrounds/` (installed to `~/.local/share/lumen/packs/backgrounds`); add that folder with "Klasörden ekle" for a ready carousel.

## Show flow

Besides debating, the Director cues live-show moments: the first turn opens the show ("Merhaba arkadaşlar…", today's question, the opponent), each host introduces themselves on their first turn, a new topic is presented to viewers before stances, and every few quiet turns the hosts turn to the chat and invite comments. Viewer messages reach the hosts with the viewer's handle so they answer by name ("Mert, güzel soru…").

## Hakikat Arayışı: sessions and the operator panel

The default concept is **Hakikat Arayışı** (`packs/personas/hakikat-arayisi.toml`): Nermin Hanım (Kemalist) and Kâmil Efendi (Kemalizm eleştirmeni) argue each question non-stop. A broadcast session is the list of questions entered on the **Sorular** page (Ctrl 2): add with Enter, click to edit, reorder, delete, set minutes per question (default 10). Total broadcast time = questions × minutes; "Yayına uygula" pushes the list to the running runtime (`SetTopics`), and it is re-applied automatically whenever the runtime starts. The list is saved per pack in `~/.config/lumen/sessions/<pack>.toml`. When the last question's time is up the runtime ends the session, pauses itself and the frame shows a closing line.

On air the frame shows only the concept name and "SORU n / N" (plus the münazara phase); no "today's question" wording and no clocks. The broadcast clock lives in the panel only: the **Oturum** card (Prova and Kontrol) shows elapsed / total time, time left on this question and on the broadcast. **Kontrol** (Ctrl 3) adds the question list with done/live markers, OBS status, the on-air transcript and activity.

The default background is a cross-fading carousel of four GPT-drawn oriental backdrops (Ottoman carpet, İznik tiles, Anatolian kilim, lattice with brass lanterns) under a 60 % veil; change or replace them on **Görünüm**. Shipped images are referenced as `lumen:backgrounds/…` so they resolve to the installed data directory or the checkout. Görünüm has a full colour picker for every theme colour (ground, gradient, text, muted text, panel, border, both sides, question card): saturation/value field, hue strip, hex entry, a 40-colour palette with oriental tones, recent colours, and a WCAG contrast check of text against ground and panel.

## OBS (Yayın page)

Lumen drives OBS Studio over obs-websocket (obws); OBS still does composition, encoding and the RTMP output to TikTok LIVE. The **Yayın** page (Ctrl+4) walks through: enable the OBS WebSocket server (done in OBS's own config while OBS is closed, keeping your password), start OBS, connect, **Sahneyi kur**, then start/stop the stream and recording with live duration, congestion and skipped-frame stats.

"Sahneyi kur" creates a separate **Lumen** scene collection (your own collections are not touched), sets 1080×1920 at 30 fps and builds scene **Münazara**:

- **Browser Source builds of OBS:** the 1080×1920 overlay page `lumend` serves at `http://127.0.0.1:47800/` (loopback only, GET only, whitelisted files; `LUMEN_OVERLAY_PORT` to change).
- **OBS without Browser Source** (e.g. Arch's `obs-studio`): a native composition of OBS colour, image, slideshow and FreeType text sources that Lumen updates live (~8 Hz): question, phase and timer, names, side labels, speaking/thinking state, viewer question and subtitles; avatar mouth/blink frames are toggled scene items, so there are no image reloads. Panels have square corners in this mode.
- **Audio:** a PipeWire/Pulse null sink `Lumen-Yayin` captured by OBS, with desktop and mic audio muted in the Lumen collection so other apps never reach the stream; "Kulaklıkta dinle" loops it to your headphones. In live mode the app routes `lumend` into this sink.

For a single destination, the stream service (RTMP server and stream key) stays in OBS ⁜ Settings ⁜ Stream and Lumen never handles the key. To stream one scene to several authorized accounts at once, use `lumen-live broadcast` (keys stored encrypted; see [docs/platforms.md](docs/platforms.md)). `cargo run -p lumen-obs --example smoke -- OUT.png` sets the scene up against a running OBS and saves a program-output screenshot (use an isolated `XDG_CONFIG_HOME` to test without touching your OBS profile).

## Shared skills: spin doctor

`packs/skills/_shared/spin-doctor.md` is a debate playbook given to both characters when a pack lists `skills = ["spin-doctor"]`. It is grounded in applause-device research on political speeches (contrast, three-part list, puzzle-solution, headline-punchline, position taking, pursuit — Atkinson 1984; Heritage & Greatbatch 1986), framing (Entman 1993; Chong & Druckman 2007) and moral foundations (Haidt 2012; Turkish validation Yilmaz et al. 2016). Every turn follows: stance in the first words, opponent's frame turned around, one concrete (explicitly hypothetical if invented) image, contrast or three-part list, a short repeatable punch. Colloquial exclamations ("be", "yahu", "ulan", "lan") are allowed at heated moments; swearing, insults, fabricated facts and quotations, and attacks on Atatürk's person (Law 5816), religious values or groups are not. Character sheets gain "Vuruş cümleleri" and "Değer çapaları" sections and are generated with the playbook as reference.

## Stage avatars

`packs/avatars/<preset>/avatar.toml` describes a look; `lumend generate-avatars packs/avatars` has GPT draw `idle.png` in a fixed serious editorial-portrait style, then edits that same image into `talk.png` (mouth open) and `blink.png` (eyes closed). The frame swaps them by live speech level (mouth flaps), blinks every few seconds and adds slight breathing motion. Agents pick a preset with `avatar = "fesli"`. Characters are fictional; looks must not resemble identifiable real people.

## Current checks

```sh
cargo test -p lumen-debate -p lumen-protocol -p lumen-storage -p lumen-audience -p lumen-personagen --target-dir target/core
cargo clippy -p lumen-debate -p lumen-protocol -p lumen-storage -p lumen-audience -p lumen-personagen --all-targets --target-dir target/core -- -D warnings
cargo fmt --all --check
```

The floor controller, bounded audience queue, IPC framing and SQLite journal are implemented, with a session coordinator for synthetic playback acknowledgments. The GPUI Studio and Bevy stage attach independently to the daemon. A configured show connects model/TTS/playback and journal code; real full-show playback, GUI rendering and broadcast validation remain pending.

```sh
cargo run -p lumen-studio
cargo run -p lumen-stage
```

These GUI commands require a working Linux graphics session. They do not start streaming or access accounts. Build concurrency is limited to two jobs to reduce memory pressure.

## Accelerated scenario

```sh
cargo run -p lumend --target-dir target/core -- simulate /tmp/lumen-simulation.sqlite 1800
```

Exercises 30 minutes of virtual time through the Director, audience queue, floor and journal. Text and playback are simulated; the command does not call a model, synthesize audio or stream. Each run writes a distinct simulation session. It checks stale callback rejection and excludes unplayed text from journaled dialogue.

## Show packs

```sh
cargo run -p lumend --target-dir target/core -- validate-pack packs/personas/history-clash.toml
```

The example defines two fictional Turkish hosts with distinct personas, assigned positions and synthetic voice presets. Model names are examples; install/configure a compatible model before connecting. Packs contain no API key values: cloud configurations may name an environment variable with `api_key_env`. Validation makes no network requests and does not read that variable.

## Persona generation (offline, `codex exec`)

Personas are authored from operator briefs in `packs/briefs/` and written to `packs/personas/`:

```sh
cargo run -p lumend --target-dir target/core -- generate-pack packs/briefs packs/personas
cargo run -p lumend --target-dir target/core -- generate-pack packs/briefs/free-will.toml packs/personas/free-will.toml --force
cargo run -p lumend --target-dir target/core -- generate-pack packs/briefs/free-will.toml /dev/null --print-prompt
```

A brief fixes the show, topic, format, tone, each agent's assigned stance, optional fixed name/character direction, and all runtime settings (voice, model, endpoint, `api_key_env`, temperature, token budget). Only names, persona and position text are drafted by the model; runtime settings never pass through it and are not even included in the prompt.

Pipeline (`crates/lumen-personagen`): brief validation (including endpoint policy) before any call -> writer prompt -> `codex exec --sandbox read-only --ephemeral --skip-git-repo-check --output-schema` in an empty temp workspace with prompt on stdin and a hard deadline -> strict draft JSON -> rule checks (lengths, distinct names/personas/positions, fixed names, no URLs/markdown/control characters) with up to 3 attempts that feed rejection reasons back -> fixed honesty/no-impersonation guardrail appended to every persona -> `ShowPack` validation and round-trip parse -> atomic write. Existing packs are skipped unless `--force`. Usage-limit/auth failures stop the batch immediately; other failures retry.

Options: `--model MODEL`, `--attempts 1..5`, `--timeout-secs 30..1800`. `LUMEN_CODEX_BIN` overrides the `codex` binary. Generation is an authoring step: it never runs during a show, and generated personas should be reviewed before broadcast.

## Topic rotation

Packs (and briefs) take an opening `topic`, a `topics = [...]` list and `topic_minutes` (default 10). The Director rotates through the list on that schedule, wrapping around; F9 / "Sonraki soru" skips immediately. Positions are written as a worldview that holds across every topic, and each agent can carry a short on-screen `side` label (max 28 characters).

## Persona skill sheets

```sh
lumend generate-skills packs/personas            # all packs, skips existing sheets
lumend generate-skills packs/personas/free-will.toml --force
```

Each character gets a `SKILL.md`-style sheet at `packs/skills/<pack>/<character>.md` (frontmatter + identity, voice, signature phrases, argument moves, humor, objection handling, audience handling, boundaries, sample lines). Sheets are drafted by `codex exec` from the pack, rendered into a fixed structure with fixed honesty/no-impersonation boundaries, and passed to the model as a style guide below the runtime rules. `serve` and `rehearse` load them by convention.

## Rehearsal room and broadcast frame

```sh
lumend rehearse packs/personas/history-clash.toml        # scripted lines, no model needed
lumend rehearse packs/personas/history-clash.toml --models
cargo run -p lumen-studio -- --rehearsal                 # frame + operator panel
cargo run -p lumen-studio -- --stage                     # clean 9:16 frame for OBS capture
```

`rehearse` runs the real Director, floor, journal, topic rotation and audience ingress with no TikTok connection, no TTS and no audio device; speech timing is simulated from text length. Without `--models` characters speak their skill-sheet sample lines. The rehearsal window shows the broadcast frame next to an operator panel (start/pause, next question, hand over the floor, emergency stop, simulated viewers, typed comments that enter the real question queue). Studio also opens it from its header.

The broadcast frame is designed for all ages: one large question, two large characters with a plain side label, the active speaker highlighted (mouth moves with speech level, the other dims), and large high-contrast subtitles. The bottom third and right edge are kept clear for the platform's own chat and buttons. The frame has no "AI" banner; disclosure belongs to the platform's AI-generated content label, which must stay enabled, and characters never claim to be human.

## Local control endpoint

```sh
install -d -m 700 /tmp/lumen-runtime
cargo run -p lumend --target-dir target/core -- serve /tmp/lumen-runtime/control.sock /tmp/lumen-runtime/journal.sqlite
# In another terminal:
cargo run -p lumend --target-dir target/core -- control /tmp/lumen-runtime/control.sock status
```

CLI controls: `status`, `pause`, `resume`, `mute`, `stop` (latched emergency stop). The endpoint currently controls runtime flags only; model/audio/OBS backends are not attached. It starts paused, survives client disconnects and refuses to overwrite an existing socket path. Ctrl-C shuts down and removes its socket. After an unclean process kill, inspect/remove its stale socket before restarting.

## Audio smoke check and voice worker

```sh
cargo run -p lumen-voice --example silent-playback --target-dir target/audience
uv sync --project workers/tts-python --python 3.12
(cd workers/tts-python && .venv/bin/python -m unittest -v test_worker)
```

The hardware smoke check sends 100 ms of silence and waits for a device-clock completion. It currently requires float32 24 kHz mono support; `LUMEN_AUDIO_DEVICE` selects a device by description name. The Python worker uses a pinned Turkish model revision, synthetic presets and CPU-only dependencies; the first model load downloads weights. Speech quality and Rust worker integration are still under development.

To measure the real cached-model TTS worker without playing sound:

```sh
cargo run -p lumen-voice --example tts-benchmark --target-dir target/core
```

Reports first-chunk latency, total synthesis time and audio duration for four Turkish sentences across two presets. The first request includes process/model initialization; later requests reuse the worker. These are raw measurements, not a latency guarantee or voice-quality assessment.

Studio attaches to the same daemon socket:

```sh
cargo run -p lumen-studio -- /tmp/lumen-runtime/control.sock
```

It polls connection/control state. F2 pauses, F3 resumes, F4 mutes, F8 skips a turn, F9 skips a topic, F10 requests stream stop and Ctrl+Shift+Escape requests emergency stop. Controls also support Tab/Enter. Show and OBS actions report unavailable until their backends are connected.

To attach a configured show to the daemon:

```sh
cargo run -p lumend --target-dir target/core -- serve /tmp/lumen-runtime/control.sock /tmp/lumen-runtime/journal.sqlite packs/personas/history-clash.toml
```

Starts paused. Resume runs the two configured agents through streaming TTS and local CPAL playback; Pause, Mute and Emergency Stop cancel the active turn. Mute toggles; unmute before resuming. Skip Turn cancels the active response when a show is attached. Provider failures pause the show for operator recovery. A compatible model endpoint and the voice worker environment must be available. This integration remains under validation; content moderation, audience ingestion, stage captions and OBS are not connected yet.

The stage reads the same socket independently:

```sh
cargo run -p lumen-stage -- /tmp/lumen-runtime/control.sock
```

It displays configured names/topic, active captions, speaker highlights and output-audio-driven mouths in a 1080×1920 scene. Runtime disconnect clears active speech while the scene keeps rendering. Protocol version 2 requires rebuilding daemon, Studio and Stage together. Capture and AV timing calibration remain under validation.
