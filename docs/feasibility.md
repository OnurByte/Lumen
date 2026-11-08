# Feasibility evidence

Checked 2026-11-06 on the target machine.

- Empty repository, no inherited application code or data.
- rustc 1.99.1 and Cargo 1.99.1 installed.
- `/usr/bin/obs` and `/usr/bin/pipewire` installed. Installation alone does not establish capture or routing.
- Cargo registry search confirms Bevy 0.20.1 and GPUI 0.2.2 exist. Compile and runtime validation pending.
- Full specification preserved in `LUMEN_PLAN.md`.

## Pending gates
Independent GUI launch, IPC, OBS capture, PipeWire routing, Turkish voice quality/latency, connector access, license inventory, simulated endurance and supervised broadcast remain unverified.

## Core increment
`cargo test --workspace` passes four floor-controller integration tests: canceled/stale playback rejection, exclusive floor and ordered acknowledgments, pause/emergency stop, and bounded/validated speech queues. This is an in-memory controller; it does not yet establish durable playback, actual audio cancellation or end-to-end runtime behavior.

`cargo fmt --all` could not run: cargo-fmt missing from selected stable toolchain. `rustup` is not on PATH. Clippy has not yet run.

`cargo info` reports GPUI 0.2.2 Apache-2.0 and Bevy 0.20.1 MIT OR Apache-2.0, with Bevy requiring Rust 1.97.1. PipeWire inspection confirms a running Wayland session and existing OBS audio connections; Lumen routing remains pending.

## Independent GUI spikes
Added separate `lumen-studio` (GPUI =0.2.2) and `lumen-stage` (Bevy =0.20.1) binaries. They are launch spikes, not functional broadcast interfaces. Stage requests a 1080x1920 window; actual rendering/capture remains unverified. Cargo dependency check started in terminal session 93172; inspect that session before restarting it.

Located rustup at `/home/borat/.cargo/bin/rustup` and installed stable rustfmt/Clippy components. `cargo fmt --all --check` now passes. Earlier missing-tool limitation is resolved; Clippy execution remains pending.

## Protocol increment
Added `lumen-protocol`: versioned MessagePack operator commands over length-prefixed streams, with a 64 KiB frame ceiling, strict version validation and rejection of trailing payload data. Tests cover roundtrip, actual Unix socket pair transport, oversized headers, malformed/truncated frames and unsupported outgoing versions. Test execution pending at this entry.

The initial graphics dependency build suffered compiler SIGKILLs. Retrying with `.cargo/config.toml` limiting build jobs to two (session 61957). This is not yet a successful GUI compile. Core tests now use `--target-dir target/core` to avoid waiting on the graphics build directory lock.

Verification update: `cargo test -p lumen-protocol -p lumen-debate --target-dir target/core` passed all eight integration tests. `cargo fmt --all --check` passed. This proves the framing and in-memory floor behaviors, not the daemon or end-to-end playback.

## SQLite journal
Added SQLx 0.8.6 SQLite storage (MIT OR Apache-2.0 per Cargo metadata), WAL mode, full synchronous writes, busy timeout, append-only triggers and indexed session/event pagination. Two integration tests pass for reopen durability, ordered replay, session isolation, audible-only projection and invalid input rejection. Playback and journal are not yet connected; event identity/schema and runtime integration remain work.

`cargo clippy -p lumen-protocol -p lumen-debate --all-targets --target-dir target/core -- -D warnings` passed. Storage Clippy is pending. Graphics check session 61957 remains active. Initial storage dependency compilation exited 137; single-job retry completed and both tests passed.

## Playback/journal integration
Added a session coordinator that validates ordered/current-turn acknowledgments before persistence, writes played text durably before consuming pending speech, invalidates speech immediately on cancellation and pauses on journal failure. All 12 floor/protocol/storage/session integration tests passed together. The new session tests verify interruption/reopen and duplicate/out-of-order rejection. This still uses synthetic acknowledgments, not a real audio backend. Playback metadata identities and restart generation isolation remain pending.

## Audience queue
Added data-only audience queue with 128 pending comments, 4096 retained dedup/rate-limit records, 60-second expiry, 1000-byte text limit, 128-byte identities, 2-second per-user admission cooldown and distinct-user rotation. Overload rejects new work rather than evicting dedup history. Four integration tests and strict Clippy passed. This is not yet connected to TikTok, command eligibility, topic relevance, operator moderation or the Director.

## Restart identity and graphics dependency correction
A new regression test demonstrated fresh floors reused `TurnId(1)`, allowing stale callbacks to match a new runtime. Turn IDs now use UUID v4; all eight floor/session tests pass, including restart rejection and persistence-failure pause. UUID identity does not influence deterministic Director choices.

The combined graphics check failed in transitive `xattr 0.2.3`, reached through GPUI -> gpui_http_client -> zed-async-tar. It references Linux `libc::ENOATTR`, absent in libc 0.2.191. Inspected registry source confirms libc 0.2.186 retains the ENODATA alias. Studio now pins libc =0.2.186 with a removal condition; compatibility build pending. Separate stage check session 8442 was started to isolate Bevy from this failure.

## Director and scenario runner
Added deterministic two-agent Director with validated identities/topic, Free Debate (30-second brief ceiling), Crossfire (12-second ceiling), pause/resume and separately typed audience question data. Three tests pass, including repeated-input decision equality and pause preserving the audience queue.

`lumend simulate JOURNAL_PATH SECONDS` now drives the real Director, audience queue, floor/session and SQLite code with explicitly synthetic generated text and playback. The 1800-second virtual-time test completes 120 turns, rejects 24 injected stale callbacks and verifies 120 journaled audible segments with no unplayed text. CLI rejects invalid durations. This is an accelerated invariant check, not 30 minutes of real provider/audio endurance and not completion of the full Phase 1 gate.

`cargo test -p lumend -p lumen-debate --target-dir target/core -j 1` passes 13 tests. Matching strict Clippy and workspace formatting checks pass. Model providers, actual audio/TTS, daemon IPC server and GUI integration remain pending.

Graphics update: `cargo check -p lumen-stage` completed successfully with Bevy 0.20.1. This is a type/dependency check only; linking, window launch, 30 FPS capture and synchronization remain unverified. Studio session 33821 is rebuilding with the libc compatibility pin.

## Streaming provider
Added custom `ModelProvider` trait and OpenAI-compatible streaming adapter with request-specific persona/position/audible context, audience in labeled user data, no tools, HTTPS except loopback, disabled redirects, connect/request deadlines, bounded events (64 KiB) and output (8 KiB), cancellation during network reads and bounded-channel sends. Four local HTTP fixture tests passed, including byte-split Turkish UTF-8 and cancellation of a hanging stream. Strict provider Clippy is running in session 68570. Actual account endpoints, provider quality/latency, segmentation/moderation and agent-loop integration remain unverified.

## Show configuration
Added strict, size-limited TOML show packs with two independently configured agents (persona, position, voice, endpoint/model, temperature/token budget and optional API-key environment reference). Built-in History Clash uses fictional Turkish characters and separates their assigned stance from persona. Pack tests and existing provider fixtures pass. A `validate-pack` CLI path was added; full regression/Clippy checks are running. Configuration is not yet wired into an actual debate loop, so temperature/voice selection are not yet operational.

Local probe: no `ollama` executable found on PATH and no response from localhost:11434 within two seconds. This is not a blocker for fixture-based development, but no live local model has been validated.

Verification update: all 30 core/CLI integration tests pass together (`cargo test -p lumend -p lumen-debate -p lumen-audience -p lumen-protocol -p lumen-storage --target-dir target/core -j 1`). Strict Clippy for lumend/debate and their dependencies passes, including corrected provider test read handling. Pack CLI validation test passes without network or secret access.

## Daemon control endpoint
Added `lumend serve SOCKET JOURNAL`: persistent Tokio Unix socket endpoint with private-parent requirement, mode-0600 socket, 16-connection bound, 2-second request deadline, versioned MessagePack status responses and control-event journaling. Tests pass for reconnect independence, pause/resume/mute flags, latched emergency stop and refusing an existing path without deletion. Unsupported show/OBS controls explicitly return an error. No generation/audio/show backend is attached yet; this does not prove emergency stop of actual pending model/audio work.

Both GUI cargo checks have now passed. GPUI's transitive proc-macro-error2 2.0.1 warns about re-exporting private proc_macro, which may fail under future Rust releases; current Rust accepts it.

Control verification: all 10 lumend/protocol tests pass together, and matching strict Clippy plus formatting pass. CLI supports control status/pause/resume/mute/stop with socket I/O deadlines and nonzero failure status. Daemon integration is still a control-plane increment; active show cancellation, hardware audio and OBS behavior are not yet implemented.

## PCM arbitration
Added `lumen-voice` arbiter with exclusive utterance ownership, generation/utterance and chunk-sequence checks, fixed format validation, finite normalized sample validation, 32-frame/two-second queue ceilings and immediate queued-audio cancellation. Four tests pass for partial cancellation, exclusive floor, final-chunk completion, invalid samples/sequences and overload. These are buffer-consumption tests: no device playback or audible-journal acknowledgment is claimed.

GUI linking build started in terminal session 56276 (`cargo build -p lumen-studio -p lumen-stage -j 1`). Both targets had previously passed cargo check; executable launch and capture remain pending.

PCM verification update: five arbiter tests pass, including the two-second sample budget and mismatched sample-rate rejection. Strict Clippy passes after using the standard integer divisibility method. CPAL playback timestamps, TTS transport, real audio cancellation and journal attachment remain pending.

## CPAL and isolated TTS worker
Added CPAL 0.17.3 output for float32 24 kHz mono, device selection, bounded completion/error channel and device-clock completion fences. Initial silent hardware test timed out: trace showed ALSA timestamps switching from absolute to stream-relative after initial callbacks. Output now waits for three stable callbacks, rebases on clock regression and cancels pending audio/fences on reset. The silent-playback hardware example now passes on this machine. Six Rust voice tests and strict Clippy pass. This proves stream/timing smoke behavior, not speech quality, calibrated capture sync or cancellation of downstream device buffers already handed off.

Installed uv-isolated Python 3.12 environment with CPU-only PyTorch, pocket-tts 3.3.0 and pocket-tts-turkish 0.1.0; uv.lock preserved. Import succeeds. Worker validates bounded JSON requests and emits separate binary float32 PCM frames with generation, utterance, sequence, format and final-chunk metadata. Two standard-library Python tests pass. Model revision pinned to fcbd37c74dc127be846face0ff36062e9a939f7b. Real model download/synthesis started in session 9047, output /tmp/lumen-tts-smoke.pcm; inspect that session before restarting. Rust worker transport and end-to-end playback integration remain pending.

## Rust TTS worker transport
Added `VoiceProvider` contract (`synthesize_stream`, `cancel`, `list_voices`, `capabilities`, `health_check`) and persistent isolated Python adapter. Binary frame limits, exact generation/utterance/sequence, normalized finite samples and a 30-second utterance ceiling are enforced before admission. Cancellation/error kills and reaps the worker; the next request starts a new process. Cold/model operations have a 120-second deadline. Health reports process liveness separately from having produced model audio.

All nine Rust voice tests pass, including malformed/oversized/stale PCM and hanging-worker cancellation/restart. Actual first Turkish synthesis completed: validated 39 frames, 3.190 seconds of 24 kHz mono audio, proper final marker. Speech quality has not been listened to or judged. Persistent-worker benchmark running through the Rust adapter; model download is excluded from its warm-cache measurements. GUI linking build session 56276 remains active.

Worker transport verification update: all nine Rust voice tests and strict Clippy pass after fixed-size PCM decoding cleanup. Benchmark session 90856 is actively generating through the real persistent worker (observed CPU work, warm model cache); output is /tmp/lumen-tts-benchmark.json and will appear when all cases finish. No benchmark numbers have been claimed yet.

Actual TTS benchmark completed successfully; raw results saved to `docs/tts-benchmark.json`. Ryzen 5 6600H, CPU-only PyTorch, two TTS threads, warm model cache, with concurrent GUI compilation. First request includes model/process/voice setup; first use of the second preset also includes its preparation. Each total synthesis duration exceeded its generated audio duration, so this run does not meet sustained real-time production. These four cases cannot establish p95 latency or voice quality. Need isolated-load retest, voice prewarm, native/GPU candidate comparison and listening review before selecting the production adapter.

## Studio control room increment
Replaced Studio launch placeholder with connection/status card, periodic polling, required Pause/Mute All/Skip Turn/Skip Topic/Stop Stream/Emergency Stop controls, Resume and keyboard shortcuts plus Tab/Enter activation. IPC runs on bounded background threads, with capacity reserved for emergency stop; closing Studio drops its client only. Shared protocol client is used by both CLI and Studio. Unsupported daemon backends still return explicit errors; only control flags are currently operational.

Studio cargo check passes. All 12 daemon/protocol tests pass, including shared client request/response and disconnect failure. GUI strict Clippy session 60155 is running. The older GUI linking build 56276 was deliberately interrupted after the Studio dependency/source change; its compiler artifacts are retained, but no GUI executable launch is claimed. Updated linking build queued after Clippy.

## Streaming turn pipeline
Connected model deltas -> bounded UTF-8/Turkish-aware segments -> isolated streaming TTS -> backpressured PCM -> matching playback completion -> durable session journal. Turn cancellation/deadline drops generation, clears playback and kills/reaps the worker. Successful turns finish separately from cancellation. Segment validation rejects control characters and excessive text; this is not a content-moderation adapter.

Two pipeline integration tests and two segmenter tests passed: delivered segments survive in SQLite, cancellation during hanging TTS writes no generated dialogue, and segmentation respects common Turkish abbreviations/dates and byte boundaries. External providers/audio are fixtures in these tests. Real pipeline-to-daemon attachment, content moderation and stage captions are still pending. Strict expanded Clippy is running.

Pipeline verification update: 23 debate integration tests and strict Clippy pass. New withheld-playback test confirms generated/consumed PCM without completion is not dialogue. A regression exposed premature turn_finished journaling while unplayed segments remained; validation now precedes journal write, and its test passes. All verification uses fixture model/voice/playback except the separately documented hardware smoke and TTS benchmark.

## Configured daemon show backend
`lumend serve SOCKET JOURNAL [PACK]` now optionally attaches the actual two-agent turn pipeline on a dedicated thread/current-thread Tokio runtime, keeping control requests independent from pending model/TTS work. Configured model endpoints, secrets referenced by environment name, per-agent temperatures/token budgets and voice presets are used. Shows start paused; active-turn cancellation is propagated from operator controls. Startup readiness prevents Resume after backend initialization failure. Provider/turn errors pause for operator recovery; rejected controls do not cancel a show. Journal reads for model context are bounded to the latest 32 played segments.

The daemon show-loop test passes with a hanging local HTTP model fixture: operator pause cancels the request and audible history stays empty. Storage tests prove bounded chronological recent context. Provider fixture verifies configured token budget is sent. Full lumend/debate/storage suite passed before the final readiness/recovery changes; final regression/Clippy rerun pending. No live model conversation or real complete-show playback has been claimed.

Configured-show verification update: all 33 lumend/debate/storage tests pass after readiness and recovery changes; strict Clippy and formatting pass. Studio strict Clippy also completed successfully (transitive future-Rust warning remains). Actual full live-model/TTS/playback operation is still unverified; model endpoint is unavailable locally. GUI executable linking session 14147 continues.

## Stage snapshots and scene
Protocol bumped to version 2 for bounded stage snapshots: configured agent names/topic, current speaker, caption, optional audience question and normalized output-audio level. The daemon wraps playback to present captions when first PCM is queued, clear speech on matching completion/cancel and read RMS level from CPAL output buffers. Caption speech segments are capped at 256 bytes. Tests verify snapshot roundtrip/bounds and cancellation clearing speaker/caption/level; full core regression passed before the final scene cleanup.

Replaced Bevy placeholder blocks with two code-drawn stylized characters, current-speaker highlights, audio-level mouth movement, configured names, topic and caption/question cards. Stage polls the private daemon endpoint in a separate thread and clears speaking state on disconnect. Both stage/Studio compilation checks are running in session 24790. This does not establish visual layout, frame rate, capture or calibrated AV synchronization. The obsolete GUI linking build 14147 was interrupted after the Stage dependency changed; artifacts retained, new executable linking still needed after compile.

Persona generation: added `lumen-personagen` and `lumend generate-pack` (brief -> `codex exec` structured draft -> validated, guardrailed `ShowPack`). The drafting model supplies only names/persona/position; runtime settings come from operator briefs. 10 tests cover hardened `codex exec` flags against a fake binary, deadline kill, usage-limit classification, retry-with-reasons, guardrail append, brief/endpoint rejection, atomic no-clobber writes and shipped brief validity. Generated packs are drafts for operator review, not evaluated for on-air quality.

Topic rotation, skill sheets and rehearsal room: Director rotates a pack topic list every `topic_minutes` (tested schedule, wrap and skip); F9 skip-topic is wired to the show loop. `lumend generate-skills` drafts per-character skill sheets via `codex exec` (tested render/parse/retry). `lumend rehearse` runs the real show loop with scripted lines or pack models, simulated speech timing and no TTS/audio/TikTok; a daemon test verifies captions with mouth levels, topic skip and that only played text is journaled. Studio gained a 9:16 broadcast frame (`--stage`) and rehearsal room (`--rehearsal`), verified by screenshots with simulated viewer questions reaching the stage. The Bevy `lumen-stage` scene predates this frame and does not show side labels or topic timers.

GPT characters, avatars and installer: packs can select `provider = "codex"` so turns come from GPT through the signed-in codex CLI (isolated `codex exec`, read-only sandbox, low reasoning effort, 45 s deadline, killed on cancel); measured about 8 s per turn. Turn deadlines now include generation slack and a single over-long turn is dropped instead of pausing the show; a 75 s live rehearsal of Cumhuriyet Münazarası alternated speakers, answered a simulated viewer question and journaled only played lines. `lumend generate-avatars` drew serious portrait frames (idle/talk/blink) via codex image generation; the frame swaps them by speech level. `install.sh` (Linux only) was verified end to end into a scratch prefix (release build, binaries, launcher, desktop entries, packs, uninstall). Voice/TTS quality, TikTok platform behaviour and long-run GPT latency under load remain unverified.

OBS integration: `lumen-obs` (obws) enables obs-websocket in OBS's config, builds a dedicated "Lumen" scene collection at 1080×1920/30 fps with scene "Münazara", a PipeWire null sink for show-only audio, and stream/record control with status. Verified end to end against an isolated OBS 32.2.2 profile (obs-websocket 5.7.4): the installed OBS has no Browser Source, so the native composition was built (34 sources) and driven from a live GPT rehearsal; a screenshot of OBS's program scene showed the live topic, phase/timer, speaking host, dimmed opponent and subtitle. Setup is idempotent (second run recreates without duplicates). The HTML overlay page served by `lumend` was verified in a browser with live state. Not verified: an actual RTMP stream to TikTok, audio capture through the null sink during live TTS, and Browser Source mode inside an OBS build that has it.
