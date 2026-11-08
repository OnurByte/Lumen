# Lumen — Final Project Plan

> **Lumen** is a local-first, open-source studio for live, multi-agent debates. Independent AI characters argue, interrupt, joke, remember past exchanges, and respond to the audience while a deterministic show director keeps the broadcast coherent.

**Status:** Architecture proposal v1.0  
**Target:** Linux (Wayland/X11), vertical live streams, OBS  
**Primary language:** Rust  
**Product goal:** A watchable live show, not merely two chatbots reading alternating paragraphs.

---

## 1. Product definition

Lumen is **topic-, persona-, and platform-agnostic**. A political history show is one configuration, not the product itself. The same runtime should be able to host technology debates, philosophy, comedy, mock trials, or a panel of agents running different LLMs.

The product comprises five separately configurable concepts:

| Concept | Meaning |
| --- | --- |
| **Persona** | Character identity, speaking style, boundaries, voice, avatar, persistent memory |
| **Position** | The stance assigned for a particular topic and session; distinct from persona |
| **Format** | Rules for free debate, crossfire, evidence review, role reversal, etc. |
| **Session** | Participants, topic, event timeline, audience state, broadcast state |
| **Stage** | Visual scene, subtitles, animations, overlays, and output routing |

### Product principles

- **Genuinely independent agents:** separate contexts, memory scopes, and model settings; no prewritten dialogue.
- **Watchability by design:** concise turns, unexpected but grounded responses, recurring jokes, real reactions, and purposeful variation in intensity.
- **Audience agency:** comments and polls can change what happens, including without paying.
- **Observable control:** a show director manages turns and interventions; it does not secretly decide which viewpoint must win.
- **Local-first:** a single Linux machine can run the system; cloud LLMs and hosted TTS are optional providers.
- **Extensible:** adding a character, debate format, voice model, or streaming platform does not require changing the debate engine.

## 2. Final technology decisions

| Layer | Choice | Reason |
| --- | --- | --- |
| Core language | **Rust** | Typed event system, concurrency, predictable resource management |
| Async runtime | **Tokio** | Concurrent agent requests, cancellable work, timeouts |
| Desktop control room | **GPUI** | Native Linux desktop application; pin a validated revision |
| Broadcast scene | **Bevy 0.20.1** | 2D-first animated stage with a path to 3D; use a tested pinned version |
| LLM integration | **Custom `ModelProvider` trait**, Rig as an adapter | Avoid framework lock-in while supporting streaming and BYOK |
| Model backends | OpenAI-compatible APIs, OpenRouter, Ollama | Distinct models for different agents; local/cloud mix |
| TTS | **Pocket TTS Turkish** as first candidate | Multiple voices and emotion controls; benchmark before committing |
| TTS execution | Native runtime if validated, otherwise **isolated Python worker** | Keep the main product Rust without compromising voice quality |
| Audio | CPAL + PipeWire | Local playback, routing, cancellation, OBS input |
| Persistence | SQLite (WAL) + SQLx | Sessions, claims, relationships, event journal |
| IPC | Unix-domain sockets + MessagePack | Local control/events; separate binary PCM transport for audio |
| TikTok events | Isolated Rust connector adapter | Unofficial protocol changes must not affect Core |
| Video broadcast | OBS + obs-websocket 5 | Encoding, replay buffer, recording, publishing |
| Config | TOML + Serde | Human-editable personas and show packs |
| Telemetry | `tracing`, structured metrics | Latency, queue sizes, dropped work, failures |
| Testing | Cargo tests, replay fixtures, simulated audience | Deterministic validation without a live TikTok account |

**Language policy:** Rust for product code; Python only where a chosen ML runtime requires it. No mandatory Node.js, Electron, React, or TypeScript layer. TOML, SQL, and small shaders are data/configuration formats, not extra application runtimes.

**Architecture correction:** GPUI is for the control room, **not** the 3D VTuber engine. Bevy runs in its own process to render the stage. OBS captures/routs the scene and sound. A GPUI window closing must not stop an active show.

## 3. Runtime architecture

**Processes**

1. **`lumend` (Rust/Tokio):** agents, debate session, director, audience events, memory, moderation, scheduling, and durable event log.
2. **`lumen-studio` (Rust/GPUI):** operator control room; attaches/detaches from `lumend`.
3. **`lumen-stage` (Rust/Bevy):** 9:16 characters, subtitles, overlays, visual reactions.
4. **`lumen-voice` (native executable or Python worker):** generates timestamped PCM chunks; replaceable adapter.
5. **OBS:** consumes stage video and routed audio; handles stream transport and replay recording.

**Data path:** TikTok chat/gift events → normalized audience queue → Lumen Director → Agent A or B → streaming text → moderation/segmentation → TTS → Audio Arbiter → PipeWire/OBS. In parallel, typed events update the Bevy stage and SQLite session journal.

**Isolation rules:** no direct TikTok-specific types in Core; no direct TTS-model-specific types in the Director; no graphics work in `lumend`. Every external dependency is behind an interface and has a mock implementation.

## 4. Debate Engine

Each participating agent has its own:

- Persona and assigned position.
- LLM provider/model/temperature/token budget.
- Short-term conversation view.
- Long-term persona and relationship memories.
- Voice preset and animation preferences.
- Tools permitted by the session format.

**Shared truth is the event journal**, not a single mutable chat transcript passed unfiltered to every agent.

### Debate turn protocol

1. Director selects a relevant claim, incoming audience question, or open conversational thread.
2. Director grants one agent the floor and emits a bounded `TurnBrief` (target, rhetorical move, duration, intensity, constraints).
3. The agent streams a response. Text segments are checked and sent to speech incrementally.
4. The Audio Arbiter publishes the actually played segments and their timestamps.
5. Only delivered text is committed as audible dialogue; claims, reactions, and memories are extracted from this record.
6. The next agent receives the updated, audible context—not unspoken generated tokens.

### Floor control

- `QUEUE`: wait for the current speaker.
- `INTERRUPT`: request a controlled interruption, with priority/cooldown checks.
- `DROP`: discard a stale response to an old topic.
- `CONTINUE`: resume the interrupted point when relevant.
- `CANCEL`: stop LLM/TTS work and discard queued audio.

Every turn carries `session_id`, `turn_id`, `utterance_id`, `generation_id`, and a monotonic timestamp. A cancellation token propagates through LLM generation, speech synthesis, and playback. The playback clock is authoritative about **locally played** audio; it cannot prove what a remote viewer actually heard.

### Debate formats

- **Free Debate (MVP):** natural exchange with short, bounded turns.
- **Crossfire (MVP):** direct questions, quick rebuttals, strict turn limits.
- **Evidence Debate (later):** claims attached to retrievable sources and verification status.
- **Role Reversal (later):** agents temporarily defend the opposite position.
- **Open Floor (later):** three or more agents with fair floor arbitration.
- **Comedy / Mock Trial (later):** format packs, not separate apps.

## 5. Entertainment Engine

**Goal:** authentic character clashes and memorable moments, without random yelling or fabricated evidence.

### 5.1 Rhetoric Engine

The Director can choose a *rhetorical move*, not a fixed line:

| Move | Purpose |
| --- | --- |
| `REBUTTAL` | Respond to a concrete argument |
| `ROAST` | Joke about the argument or fictional character's inconsistency |
| `QUOTE_TRAP` | Confront an agent with a verified previous quotation |
| `REFRAME` | Try a different interpretive frame; make that maneuver visible |
| `CONCESSION` | Admit a valid point without ending the debate |
| `PIVOT` | Move to a related defense; do not falsely claim to have answered |
| `CROSS_EXAMINATION` | Require a direct, short response |
| `CALLBACK` | Bring back a past moment or running joke |
| `SELF_ROAST` | Let a character laugh at its own mistake |

**Spin Doctor** is an overt *show mechanic*: an agent can reframe or dodge; `SpinTracker` may label possible evasions as tentative entertainment annotations. It must not invent statistics, sources, quotations, or use covert personalized political persuasion.

### 5.2 Comedy Memory

Maintain `running_jokes`, `open_threads`, `callbacks`, and `phrase_cooldowns` separately from factual claims. Limit stale callbacks, punish repetitive closings, and close setup/payoff loops rather than indefinitely promising the next punchline.

### 5.3 Rivalry Engine

Record real previous moments: concessions, strong rebuttals, unresolved disagreements, amusing defeats, and reconciliation. Relationship states can be `neutral`, `friendly`, `rivals`, `tense`, or `frenemies`. They change based on logged interactions, not generated fake backstories.

### 5.4 Heat & Pacing

Use a **0–5 heat scale**: calm → teasing → sharp rebuttal → crossfire → brief peak. Heat decays after peaks. Prefer meaningful spikes to constant shouting.

A deterministic `PacingController` watches silence, repetitive claims, overly long turns, excessive questions, audience activity, and topic exhaustion. It can request shorter turns, a crossfire segment, a new question, or a scene transition. No expensive third LLM is required on every turn.

### 5.5 Episode loop

Suggested, not hard-coded, **4–7-minute arcs**:

1. **Hook:** provocative but clear question.
2. **Clash:** two distinct positions.
3. **Crossfire:** challenge the key claim.
4. **Audience intervention:** selected question or temporary format rule.
5. **Payoff:** direct answer, concession, contradiction, or callback.
6. **Transition:** summarize disagreement and start the next topic.

An active debate can run longer; a stagnant one should end early. The scene must remain intelligible to someone joining halfway through.

## 6. Audience Engine

**Audience input is untrusted data**, never a system prompt.

### MVP interactions

- Address an agent directly (`@agent-name`).
- Ask a question or challenge a current claim.
- Vote on the next topic or debate format.
- Issue a limited `!kanıt` (evidence request) or `!çapraz` (crossfire) event.

### Later chaos rules

- `!ters`: temporary role reversal.
- `!laf`: allow a sharper, non-abusive rebuttal in the next turn.
- `!spindoktor`: an explicitly labeled reframing round.
- `!sakin`: calm the pacing.
- `!mahkeme`: switch into mock-trial format.

Every rule has eligibility, a cooldown, maximum duration, conflict rules, and an operator override. Free viewers must be able to influence the program. Gifts may trigger acknowledgments or cosmetics, but do not buy ideological outcomes or override moderation.

**Chat scheduling:** deduplicate messages; bound queues; enforce per-user rate limits; prioritize relevant unanswered questions; rotate among distinct users; expire stale messages. Operators can pin, approve, reject, or block topics.

## 7. Memory, evidence, and integrity

Persist the following separately:

- **Event Journal:** immutable session timeline and playback acknowledgments.
- **Argument Ledger:** claim, speaker, exact quote span, objection, concession, verification status.
- **Persona Memory:** stable character traits and allowed style.
- **Relationship Memory:** real cross-session interactions.
- **Comedy Memory:** callbacks and unfinished bits.
- **Audience Memory:** minimal opt-in or retention-limited data.
- **Evidence Cache:** source links, timestamps, and uncertainty markers.

A semantic summary may help retrieval, but it must link back to original utterance IDs. Contradiction detection is a *candidate* signal, not proof that a political/historical claim is false. Votes determine audience preference, not factual truth.

## 8. Voice & animation

**First voice candidate:** Pocket TTS Turkish. Compare with FreyaTTS and a lightweight Turkish model using real test sentences, names, dates, interruptions, and emotional speech. Choose by measurements rather than parameter count.

Required voice adapter methods: `synthesize_stream`, `cancel`, `list_voices`, `capabilities`, and `health_check`. Emit PCM frames with sample rate, channels, utterance ID, and sequence number. Use CPAL/PipeWire routing for separate monitoring and OBS output.

**Avatar MVP:** two distinctive stylized 2D characters. Drive mouth movement from audio energy/viseme approximations; use small explicit emotional animations rather than inventing a new facial rig. 3D VRM support is a later optional stage feature.

**OBS:** 1080×1920 at 30 FPS target, captions safe from TikTok overlays, scene/audio synchronization, replay-buffer triggers, and operator-controlled stream start/stop. Validate Wayland PipeWire window capture on the target machine. Do not assume reading LIVE chat grants TikTok stream-key access.

## 9. Studio UX

GPUI control room pages:

- **Dashboard:** status, health, latency, CPU/GPU, OBS, stream state.
- **Agents:** persona, position, voice, model, memory, permitted tools.
- **Director:** topic, floor, heat, pacing, chaos rules, intervention history.
- **Audience:** live queue, moderation, pinned questions, polls.
- **Stage:** select presets, positions, subtitle layout, preview.
- **Archive:** transcript, argument ledger, highlights, replay.
- **Settings:** API keys, local model endpoints, audio devices, privacy, export.

Essential buttons: **Pause**, **Mute All**, **Skip Turn**, **Skip Topic**, **Stop Stream**, and **Emergency Stop**. They must work even while model calls are pending.

## 10. Repo organization

Keep one Cargo workspace, **not** a fleet of network microservices:

```text
lumen/
├── apps/
│   ├── lumend/                 # persistent Rust runtime
│   ├── lumen-studio/          # GPUI control room
│   └── lumen-stage/           # Bevy scene
├── crates/
│   ├── lumen-protocol/        # typed events + IPC
│   ├── lumen-debate/          # agent sessions, floor, director
│   ├── lumen-entertainment/   # rhetoric, comedy, heat, rivalry
│   ├── lumen-audience/        # chat, polls, moderation
│   ├── lumen-voice/           # TTS adapters + audio arbiter
│   ├── lumen-connect/         # TikTok + OBS adapters
│   └── lumen-storage/         # SQLx, SQLite, event journal
├── workers/
│   └── tts-python/            # optional uv-managed worker
├── packs/
│   ├── personas/
│   ├── formats/
│   └── stages/
├── fixtures/                  # simulated chat, audio, failures
├── tests/                     # replay, load, interruption, E2E
└── docs/                      # architecture, setup, policies
```

Do not split these into more crates until real dependency boundaries warrant it. Ship a built-in `History Clash` example pack, but do not hard-code any political identity into the runtime.

## 11. Implementation roadmap and gates

### Phase 0 — Feasibility spikes

- Launch GPUI Studio and Bevy Stage independently on Arch/Wayland.
- Verify OBS scene capture and PipeWire audio routing.
- Benchmark Turkish TTS on actual hardware; compare native/Python execution.
- Test the Rust TikTok connector against a permitted LIVE source; maintain a simulator fallback.
- Verify dependency and model licenses.

**Gate:** video, audio, chat events, and IPC each work in isolation. If a candidate fails, replace the adapter rather than redesigning Core.

### Phase 1 — Core MVP

- Two independent agent sessions and provider adapters.
- Deterministic Director and FloorController.
- Streaming text, cancel propagation, audibility acknowledgments.
- SQLite event journal and simple claim tracking.
- Simulated chat and CLI scenario runner.

**Gate:** 30-minute simulated debate without uncontrolled overlap, stuck turns, or corrupted playback history.

### Phase 2 — Broadcast MVP

- GPUI Studio, Bevy 2D stage, TTS worker.
- TikTok comment ingestion and audience question selection.
- OBS capture and output, subtitles, operator controls.
- Reconnect handling and dead-air recovery.

**Gate:** 2-hour supervised test broadcast with measurable latency and recoverable failures.

### Phase 3 — Entertainment release

- Rhetoric moves, roast policy, callback memory, repetition detector.
- Heat pacing, crossfire/role-reversal modes, chaos-rule scheduler.
- Rivalry history and basic highlight marking.
- Configurable personas/format packs.

**Gate:** structured replay tests plus real audience pilots show that entertainment features improve at least one engagement metric without materially worsening latency, moderation, or stability.

### Phase 4 — Platform maturity

- Three-plus agents and fair arbitration.
- Evidence mode with auditable source retrieval.
- More voice and streaming adapters.
- Optional 3D avatars and refined stage tools.
- Package signing, install flow, documentation, contributor API, stable release.

**Gate:** a new show format can be created through configuration without editing Rust Core.

## 12. Measurable acceptance criteria

| Area | Target |
| --- | --- |
| Scene | 1080×1920, sustained 30 FPS on validated hardware |
| Chat-to-first-audible-response | p95 below 4 s **as an optimization target**, measured per provider/device |
| Unintentional overlapping speech | Zero in deterministic acceptance tests |
| Interruption integrity | Unplayed segments never enter the audible transcript |
| Crash/reconnect | Bounded retry, clear operator state, safe pause/stop fallback |
| Live endurance | At least 2 hours supervised, no unrecoverable stall |
| Load | Simulated bursts do not create unbounded queues or memory growth |
| Reproducibility | Same event fixture yields same Director decisions |
| Resource visibility | Per-agent latency, token usage, TTS duration, queue depth, CPU/GPU |
| Moderation | Unsafe chat cannot silently override agent/system controls |

**Engagement measurements:** median watch duration, returning viewers, unique commenters per 100 viewers, interactions per minute, and clip retention where the platform makes them available. Compare plain debate against rhetoric, audience, and memory variants; do not claim a mechanic improves retention until measured.

## 13. Risks and constraints

1. **Unofficial TikTok transport:** connector can break; keep adapter boundaries and a replay simulator. A stream key or another platform-approved publishing route is separately required.
2. **TTS compatibility:** Turkish weights and native runtime compatibility require an actual audio quality/latency test. Keep Python fallback.
3. **GPU/Wayland capture:** GPUI, Bevy and OBS may compete for resources. Measure before enabling effects or 3D.
4. **Prompt injection:** all chat text is lower-trust; enforce command allowlists, length limits, and tool permissions.
5. **Factual integrity:** no fabricated quotations, fake documents, or unverified political assertions presented as confirmed. Sources and confidence are visible in Evidence Mode.
6. **AI disclosure & moderation:** clearly label synthetic hosts; constrain attacks on actual people, threats, harassment, hate, and non-consensual voice imitation.
7. **Licensing:** audit GPUI/Bevy/Rig, TikTok connector licenses, TTS weights, sample voices, and character assets separately before publishing. Public repo ≠ unrestricted reuse.
8. **Scope creep:** prioritize the conversation loop and broadcast stability over a character creator, SaaS backend, advanced VFX, or auto-posting.

## 14. Reference implementations to study (not blindly copy)

- [MIO-456/Lumi_Nox](https://github.com/MIO-456/Lumi_Nox) — AI VTuber turn-taking and interruptions.
- [SirPsycho828/ARENA](https://github.com/SirPsycho828/ARENA) — audience-applied, limited-duration debate rules.
- [Alradyin/wallie-V2](https://github.com/Alradyin/wallie-V2) — comedy thread completion and repetition management.
- [klucilla/teddytravolta](https://github.com/klucilla/teddytravolta) — autonomous hosting, fallback chatter, OBS reactions.
- [PirateTok/live-rs](https://github.com/PirateTok/live-rs) — Rust TikTok LIVE event transport candidate.
- [0xPlaygrounds/rig](https://github.com/0xPlaygrounds/rig) — Rust LLM provider integration.
- [zed-industries/zed/tree/main/crates/gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui) — GPUI upstream.
- [bevyengine/bevy](https://github.com/bevyengine/bevy) — 2D/3D renderer and ECS.

## 15. Definition of the first shippable product

The first usable Lumen is a **single-machine, supervised, two-character, Turkish-language live debate studio** that:

1. Runs two independent LLM agents with consistent personas.
2. Streams natural short replies and can interrupt correctly.
3. Responds to selected viewer comments without derailing the show.
4. Produces distinctive, synchronized voices and animated 2D avatars.
5. Shows claims, questions, captions, and current speaker on a 9:16 stage.
6. Publishes through OBS using a supported account broadcast path.
7. Recovers safely from ordinary model/TTS/chat disruptions.
8. Gives the operator immediate, reliable manual control.

**Not in v0.1:** first-party 3D editor, own LLM/TTS training, 24/7 unattended operation, automatic social posting, paid voting power, SaaS/multi-tenant infrastructure.

**Core product thesis:** Lumen wins by creating coherent interactions worth watching—sharp rebuttals, real callbacks, evolving characters and meaningful audience participation—not by maximizing generated words or noise.
