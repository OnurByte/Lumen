# Multi-platform LIVE: accounts, connectors, broadcast

Lumen reads LIVE events from any number of TikTok, Kwai Global and Likee Global accounts at the same time. It merges them into one moderated inbox for the AI hosts, and it can push one scene to several authorized accounts at once. Everything is native Rust (Tokio, reqwest, tokio-tungstenite, prost, SQLx/SQLite, tracing). FFmpeg and OBS are used only for media.

## Crates

| Crate | Role |
| --- | --- |
| `lumen-live` | `PlatformAdapter` trait, typed `LiveEvent`, `LiveManager` (one supervised task per account), `EventHub` (per-account bounded queues, fair merge, dedup), moderation, lumend bridge, mock adapter |
| `lumen-accounts` | Account Center: SQLite store, XChaCha20-Poly1305 credential encryption, groups, active account, scenes, broadcast settings, metrics |
| `lumen-platform-tiktok` | TikTok adapter (per-account cookies/proxy/region) on `piratetok-live-rs` (0BSD) |
| `lumen-platform-kwai` | Kwai **Global** adapter: Rust port of weteta/kwai-live (TCP framing, `sig`, room discovery, heartbeat, protobuf), plus Kwai Studio broadcast provisioning |
| `lumen-platform-likee` | Likee Global: web API port (lucasintel/likee, likee.cr, tailsjs/likee-api-wrapper) and LIVE events via the public web viewer's WebSocket |
| `lumen-broadcast` | One MPEG-TS ingest fanned out to N destinations, with one FFmpeg process per destination |
| `apps/lumen-live` | `lumen-live` CLI: accounts, scenes, `connect`, `broadcast`, lookups, WebSocket event feed |
| `apps/lumen-tiktok` | The old `lumen-tiktok USERNAME SOCKET` command, now built on the same adapter and supervisor |

Kwai Global (kwai.com, `*.kwai-pro.com`) is **not** Kuaishou. `kuaishou` is rejected as a platform name, and no Kuaishou endpoint is used.

## Capability matrix

`Verified` means real events or responses were received from the live service during validation on 2026-11-08. `Experimental` means implemented and fixture-tested, but not confirmed against the live service. `Unsupported` means not implemented, or no working protocol is known. `lumen-live capabilities` prints this matrix from code.

| Capability | TikTok | Kwai Global | Likee Global |
| --- | --- | --- | --- |
| Profile / creator lookup | Experimental (room lookup worked) | **Verified** | Experimental |
| Search | — | **Verified** (user search) | Experimental (`searchUser`, lookup by Likee ID) |
| Videos / trending / hashtags / comments | — | — | Experimental |
| LIVE connection | **Verified** | **Verified** | Experimental (needs the LIVE's share link) |
| Chat | **Verified** | **Verified** | Experimental |
| Join | Verified (mapped; seen in the event flow) | **Verified** | Unsupported (not in the web feed) |
| Like | Verified (mapped; seen in the event flow) | **Verified** | Experimental |
| Viewer count | Verified (`RoomUserSeq`) | **Verified** (`watchingCount` at entry) | Experimental (`live_num` at entry) |
| Gift | Experimental (fixture-tested; streak end only) | Experimental (no gift seen) | Experimental (gift type id + count) |
| Follow / share | Experimental | Share Experimental, Follow Unsupported | Experimental |
| RTMP(S) broadcast with a key | Experimental (key from LIVE Producer) | Experimental (key from Kwai Studio) | Unsupported (no RTMP ingest known) |
| Platform issues the key and starts/stops the LIVE | Unsupported (copy it by hand) | Experimental (Kwai Studio session) | Unsupported |

The TikTok adapter reports `Experimental` for its own capabilities until `TikTokAdapter { live_verified: true }` is set. The table above records the manual validation run. TikTok's breakdown by event type was not logged, so only chat and the total (99, then 204 events) are certain.

### Validation evidence (2026-11-08)

- **Kwai Global.** The signed `/rest/o/user/search` and `/rest/o/live/v2/startPlay` calls worked on `az2-api-akpro.kwai-pro.com`, so the 2024 salt `382700b563f4` and client key are still accepted. `startPlay` returns the socket endpoints in `race.rounds[].horses[].hostAndPort`. That address was still `99.83.187.25:8888`, and Lumen now reads it from the response instead of hard-coding it. A 60-second session on a real public room received 5 chat, 9 join, 1 like and 1 viewer-count events (`cargo run -p lumen-platform-kwai --example watch -- HANDLE 60`).
- **TikTok.** `lumen-live connect` on a public news LIVE received 99 events (10 chat) in 60 seconds. It reconnected automatically after the server ended the stream. Short HTTP 403 throttles happen; they are treated as rate limits and retried with a long backoff.
- **Whole system.** Four accounts ran together for 75 seconds: TikTok live, Kwai live, Kwai offline, and Likee. TikTok and Kwai streamed while the offline Kwai account polled slowly and the Likee account (LIVE support not yet written at the time) failed alone. 14 moderated chat messages reached `lumend serve` through its audience ingress, with no ingress errors.
- **Broadcast.** A local test (`cargo test -p lumen-broadcast`) sends one encoder to two local RTMP servers at the same time, plus one dead destination. Both live outputs received the stream. The dead output failed and restarted by itself, and its stream key never appeared in logs or health output.

### Not verified, and why

- **Everything on Likee.** `likee.video`, `api.like-video.com` and Likee's download host refused connections from the validation network on every domain tried, so all Likee code is fixture-tested only. Run it from a network that can reach Likee before trusting it. LIVE events follow the public web viewer: the LIVE share link (`likee.video/sid/…`) renders `wsUrl`, `roomId` and `live_num`; the viewer opens `wsUrl`, sends `websocket`, and receives JSON arrays of `{c, data: {n, m, c}}` (c = 1/12 chat, 4 share, 8 gift, 9 like, 10 follow, 0 ended). This was read from a saved copy of that page in rightbot/LiKeE. The feed has display names but no user ids, so Lumen derives ids from names.
- **Kwai Studio provisioning.** The calls (`checkPermission`, `transcodeResource`, `createRoom`, `room`, `startLive`, `stopLive` under `studio.kwai.com/rest/o/w/live/supplier/cloudLive/`) and their request shapes come from Kwai Studio's own web JavaScript. They are not a documented public API. The endpoints answer on the live server (without a login they return `result 109`), and the whole flow is tested against a local fake Studio, but no real account has run it yet.
- **Gifts, follows and shares** on TikTok and Kwai, because none happened during the validation windows.
- **Pushing to real platform ingest servers.** This needs your own stream keys. TikTok issues them only to eligible accounts (LIVE Studio / stream-key access). Kwai and Likee RTMP ingest availability depends on the account and was not confirmed. The relay works with any RTMP(S)/SRT URL, but publishing to a given platform is your responsibility and subject to its terms.
- **Kwai frame bytes 4–11** were always zero in observed traffic, and **compression type 2 (gzip)** was never observed. Gzip handling is implemented and tested against our own encoder only.

## Account Center

```bash
lumen-live accounts add --platform tiktok --label main-tt --handle mycreator --group main
lumen-live accounts add --platform kwai --label main-kw --handle my_kwai_id --group main
lumen-live accounts secret main-tt cookie                 # value read from stdin, input hidden
lumen-live accounts set main-kw watch other_creator       # read another creator's room
lumen-live accounts list; lumen-live accounts show main-kw; lumen-live accounts use main-kw
lumen-live connect --group main --socket /run/user/$UID/lumen/lumend.sock --events-ws 127.0.0.1:9311
```

- Accounts have no count limit. Each one has its own credentials, settings, LIVE task, cancellation token, backoff, bounded queue, rate limit and health record, and removing an account deletes its credentials (SQLite `secure_delete`). Credentials are encrypted with XChaCha20-Poly1305. The 256-bit master key is in `$LUMEN_HOME/keys/master.key` (mode 0600; looser permissions are rejected). Each ciphertext is tied to its account id, platform and field name, so blobs cannot be moved between accounts.
- Secrets are never accepted as command-line arguments. `Credentials` and `SecretString` print as `[redacted]`, and FFmpeg output is scrubbed of stream keys before it is logged. One known exposure: the destination URL passed to FFmpeg is visible in the local process list while streaming (marked `shortcut:` in `lumen-broadcast`).
- `connect` prints a status line for each account and saves the last health snapshot per account (`accounts show`).

## Events, moderation and AI

Every adapter produces `LiveEvent { platform, account_id, room_id, event_id, platform_ts_ms, received_ts_ms, user, kind }`, where `kind` is one of chat, gift, like, join, follow, share, viewer_count or live_ended.

- **Isolation and backpressure.** Each account has its own bounded queue (default 512) and token bucket. When a queue is full, likes, joins and viewer counts are dropped and counted. Chat, gifts and follows wait up to 2 seconds before being dropped. The hub merges accounts fairly, so a flood on one account cannot starve the others (`tests/stress.rs`: 60 accounts at about 10k events/s each).
- **Deduplication.** Events are deduplicated by platform, room, kind and event id within a 10-minute window, which covers reconnect replays and two accounts watching the same room.
- **Recovery.** Reconnects use exponential backoff with jitter. Offline rooms are polled every 60 seconds and rate limits back off for 3 minutes. Bad credentials and unsupported features stop with a terminal `Failed` state. If an adapter panics, only that account is restarted; after 5 panics it is marked `Failed`.
- **Moderation.** Before the AI sees anything, messages are checked for length, control characters, blocked terms, blocked users, links, a per-user rate limit and repeats. Likely prompt-injection text is held for the operator instead of being sent to the AI.
- **AI.** Allowed chat, plus short gift cues, enter lumend's existing audience queue as `AudienceInput`, namespaced `platform:account:id`. The Director and agents decide which comments to answer, as before. Chat stays lower-trust data and can never issue operator commands. Lumen does not auto-post replies into platform chats.
- **WebSocket feed.** `--events-ws` serves each event as JSON on a loopback-only WebSocket for overlays and tools. Slow clients skip ahead instead of blocking the hub.

## Broadcast

How each platform gets its ingest endpoint:

- **TikTok:** accounts with PC streaming access see a server URL and stream key in TikTok's **LIVE Producer**. Store them with `accounts broadcast REF --ingest …` and `accounts secret REF stream_key`. TikTok starts the LIVE when video arrives.
- **Kwai Global:** either copy the server URL and key from **studio.kwai.com** the same way, or let Lumen create the room. To do that, log in to Kwai Studio in your browser, copy that session's Cookie header into `accounts secret REF studio_cookie`, and set a cover image with `accounts set REF studio_cover /path/cover.jpg` (jpg/png ≤ 2 MB, 720×1280 recommended). `broadcast` then creates the room, gets its push URL, starts the LIVE once video is flowing, and stops it when you press Ctrl-C. Only accounts with Kwai LIVE permission can do this.
- **Likee Global:** no RTMP ingest is known. Likee's PC route is the official **Likee Streamer** app for Windows, which can take OBS Virtual Camera as its camera; users have reported crashes with it. Lumen cannot push to Likee directly, and its broadcast step says so.

```bash
lumen-live accounts broadcast main-tt --ingest rtmps://<server from your platform> --video-kbps 4500
lumen-live accounts secret main-tt stream_key
lumen-live accounts authorize-broadcast main-tt --confirm   # you own the account / are authorized
lumen-live accounts broadcast-on main-tt
lumen-live scene set show main-tt main-kw
lumen-live broadcast --scene show --listen 127.0.0.1:9100
```

Point OBS at it with Settings → Output → Recording → Custom Output (FFmpeg), output to URL `tcp://127.0.0.1:9100`, container `mpegts`, or use any encoder that writes MPEG-TS to that address. Each destination re-encodes at its own bitrate in its own FFmpeg process with its own bounded queue. A destination that falls behind, is rejected or crashes restarts alone with backoff. Reading LIVE events never enables broadcasting. A destination is used only if it is enabled, explicitly authorized, and has both an ingest URL and an encrypted `stream_key`. Revoking authorization also turns broadcasting off.

## Runtimes

No Node.js, Python, Java or JS runtime is required. TikTok, Kwai, Likee, accounts, events, moderation, the show and broadcast are all Rust. Two optional extras remain:

- `workers/tts-python` is an optional speech engine. Without it, `lumend` runs live shows captions-only (`LUMEN_VOICE=captions` forces this).
- `apps/lumend/src/overlay.html` is an optional OBS Browser Source overlay rendered by OBS itself. The native stage is `lumen-stage` (Bevy).

## Upstream references and licenses

All protocol behaviour was reimplemented in Rust; no upstream source code is embedded or executed.

| Reference | License | Used for |
| --- | --- | --- |
| weteta/kwai-live | No license file | Wire facts only (framing, field numbers, endpoints, signing steps) |
| lucasintel/likee, likee.cr | BSD-3-Clause | Endpoint and field names; Lumen's fixtures are its own, shaped like theirs |
| tailsjs/likee-api-wrapper | ISC | `searchUser` / `getUserInfo` endpoint names and bodies |
| rightbot/LiKeE (saved Likee LIVE share page) | Apache-2.0 repo; page content is Likee's | LIVE WebSocket handshake and message codes (wire facts only) |
| Kwai Studio web app (studio.kwai.com) | Proprietary | Endpoint paths and request fields only (wire facts) |
| frostynight-furry/furryup-live-connect | MIT | Design ideas: normalization, dedup, multi-room, reconnection |
| BarryWangQwQ/UniBarrage | GPL (custom) | Architecture ideas only (adapter registry, WebSocket fan-out); no code |
| atvkh/kuaishou-live-mate | MIT | Chat-to-LLM flow ideas; its Kuaishou APIs are not used |
| piratetok-live-rs | 0BSD | TikTok WebSocket protocol (Rust dependency) |
