# Multi-platform LIVE: accounts, connectors, broadcast

Lumen reads LIVE events from any number of TikTok, Kwai Global and Likee Global accounts at the same time. It merges them into one moderated inbox for the AI hosts, and it can push one scene to several authorized accounts at once. Everything is native Rust (Tokio, reqwest, tokio-tungstenite, prost, SQLx/SQLite, tracing). FFmpeg and OBS are used only for media.

## Crates

| Crate | Role |
| --- | --- |
| `lumen-live` | `PlatformAdapter` trait, typed `LiveEvent`, `LiveManager` (one supervised task per account), `EventHub` (per-account bounded queues, fair merge, dedup), moderation, lumend bridge, mock adapter |
| `lumen-accounts` | Account Center: SQLite store, XChaCha20-Poly1305 credential encryption, groups, active account, scenes, broadcast settings, metrics |
| `lumen-platform-tiktok` | TikTok adapter (per-account cookies/proxy/region) on `piratetok-live-rs` (0BSD) |
| `lumen-platform-kwai` | Kwai **Global** adapter: Rust port of weteta/kwai-live (TCP framing, `sig`, room discovery, heartbeat, protobuf), plus Kwai Studio broadcast provisioning |
| `lumen-platform-likee` | Likee Global: web API (rechecked against the live site; originally from lucasintel/likee, likee.cr, tailsjs/likee-api-wrapper) and LIVE events via the public web viewer's WebSocket |
| `lumen-broadcast` | One MPEG-TS ingest fanned out to N destinations, with one FFmpeg process per destination |
| `apps/lumen-live` | `lumen-live` CLI: accounts, scenes, `connect`, `broadcast`, lookups, WebSocket event feed |
| `apps/lumen-tiktok` | The old `lumen-tiktok USERNAME SOCKET` command, now built on the same adapter and supervisor |

Kwai Global (kwai.com, `*.kwai-pro.com`) is **not** Kuaishou. `kuaishou` is rejected as a platform name, and no Kuaishou endpoint is used.

## Capability matrix

`Verified` means real events or responses were received from the live service during validation on 2026-11-08. `Experimental` means implemented and fixture-tested, but not confirmed against the live service. `Unsupported` means not implemented, or no working protocol is known. `lumen-live capabilities` prints this matrix from code.

| Capability | TikTok | Kwai Global | Likee Global |
| --- | --- | --- | --- |
| Profile / creator lookup | Experimental (room lookup worked) | **Verified** | **Verified** (`getProfileDetail` by Likee ID) |
| Search | — | **Verified** (user search) | Not working (Likee answers `52010 system busy`) |
| Videos / trending / hashtags / comments | — | — | **Verified**: creator videos, trending hashtags, comments. Not working: trending videos (`52010`) |
| LIVE connection | **Verified** | **Verified** | Experimental (share page verified on a real, ended room; WebSocket needs a LIVE in progress) |
| Chat | **Verified** | **Verified** | Experimental |
| Join | Verified (mapped; seen in the event flow) | **Verified** | Unsupported (not in the web feed) |
| Like | Verified (mapped; seen in the event flow) | **Verified** | Experimental |
| Viewer count | Verified (`RoomUserSeq`) | **Verified** (`watchingCount` at entry) | Experimental (`live_num` at entry) |
| Gift | Experimental (fixture-tested; streak end only) | Experimental (no gift seen) | Experimental (gift type id + count) |
| Follow / share | Experimental | Share Experimental, Follow Unsupported | Experimental |
| RTMP(S) broadcast with a key | Experimental (key from LIVE Producer) | Experimental (key from Kwai Studio) | Experimental (only if Likee gives you a URL and key; no public way to get one is known) |
| Platform issues the key and starts/stops the LIVE | Unsupported (copy it by hand) | Experimental (Kwai Studio session) | Unsupported |

The TikTok adapter reports `Experimental` for its own capabilities until `TikTokAdapter { live_verified: true }` is set. The table above records the manual validation run. TikTok's breakdown by event type was not logged, so only chat and the total (99, then 204 events) are certain.

### Validation evidence (2026-11-08)

- **Kwai Global.** The signed `/rest/o/user/search` and `/rest/o/live/v2/startPlay` calls worked on `az2-api-akpro.kwai-pro.com`, so the 2024 salt `382700b563f4` and client key are still accepted. `startPlay` returns the socket endpoints in `race.rounds[].horses[].hostAndPort`. That address was still `99.83.187.25:8888`, and Lumen now reads it from the response instead of hard-coding it. A 60-second session on a real public room received 5 chat, 9 join, 1 like and 1 viewer-count events (`cargo run -p lumen-platform-kwai --example watch -- HANDLE 60`).
- **TikTok.** `lumen-live connect` on a public news LIVE received 99 events (10 chat) in 60 seconds. It reconnected automatically after the server ended the stream. Short HTTP 403 throttles happen; they are treated as rate limits and retried with a long backoff.
- **Likee web API (2026-11-08).** Against the real service: profile of `Likee_Official` (13.39M followers), a missing user reported as not found, creator videos through the anonymous `getGuestInfoForPC` session, trending hashtags and video comments. A local DPI-bypass tool (zapret) had been causing the earlier TLS failures; Likee is reachable with it off.
- **Whole system.** Four accounts ran together for 75 seconds: TikTok live, Kwai live, Kwai offline, and Likee. TikTok and Kwai streamed while the offline Kwai account polled slowly and the Likee account (LIVE support not yet written at the time) failed alone. 14 moderated chat messages reached `lumend serve` through its audience ingress, with no ingress errors.
- **Broadcast.** A local test (`cargo test -p lumen-broadcast`) sends one encoder to two local RTMP servers at the same time, plus one dead destination. Both live outputs received the stream. The dead output failed and restarted by itself, and its stream key never appeared in logs or health output.
- **Broadcast CLI, end to end (2026-11-08).** `broadcast --source test` pushed one card to Kwai, Likee and TikTok accounts at once, all aimed at local RTMP servers. Each server received 16 s of H.264 720×1280 yuv420p with AAC, and no stream key appeared in the logs. `broadcast --check` against the real `studio.kwai.com` with an invalid session returned the expected "session is missing or expired".

### Not verified, and why

- **Likee LIVE events.** The real share page (`likee.video/sid/…`) still has the `wsUrl`, `roomId` and `live_num` variables Lumen reads, checked on a real ended room, where `wsUrl` is empty. No LIVE in progress has been connected, so the WebSocket handshake and message codes come only from a saved copy of that page in rightbot/LiKeE. Likee has no public list of rooms: run it with the share link of a LIVE in progress (`accounts set REF share_url …`). The feed has display names but no user ids, so Lumen derives ids from names.
- **Likee search and trending videos.** The endpoints are still in Likee's web bundle but answered `52010 system busy` in every attempt on 2026-11-08, with and without a guest session. Likee's website has dropped those pages (`/@user`, `/trending` redirect to the landing page). Hashtag videos (`close api`) and user-by-uid (`invalid op`) were removed from Lumen.
- **Kwai Studio provisioning.** The calls (`checkPermission`, `transcodeResource`, `createRoom`, `room`, `startLive`, `stopLive` under `studio.kwai.com/rest/o/w/live/supplier/cloudLive/`) and their request shapes come from Kwai Studio's own web JavaScript. They are not a documented public API. The endpoints answer on the live server (without a login they return `result 109`), and the whole flow is tested against a local fake Studio, but no real account has run it yet. The permission gate, per-account `kpn`, open-room reuse and error codes were rechecked against that JavaScript on 2026-11-08 (`checkPermission` returns `hasPermission` and `userInfo.{ksId,kpn,country}`). Pull-Stream rooms (`streamType=1`) are not implemented: they need a stream URL Kwai can reach.
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
- Secrets are never accepted as command-line arguments. `Credentials` and `SecretString` print as `[redacted]`, and FFmpeg output is scrubbed of stream keys before it is logged. One known exposure: the destination URL passed to FFmpeg is visible in the local process list while streaming (marked `shortcut:` in `lumen-broadcast`). FFmpeg has no way to read an output URL from a file or descriptor (checked on FFmpeg 9), so on a shared machine run Lumen under its own OS user.
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
- **Kwai Global** (global kwai.com, not Chinese Kuaishou): see "Going LIVE on Kwai" below.
- **Likee Global:** Likee gives no public way to get an ingest URL or key. The Likee Streamer 1.0.3.9 binary has "URL:" and "Stream Key:" labels and an `rtmp://%1:%2/live` template, so RTMP exists inside it, but where a key comes from is unknown; its login is an unfinished binary protocol (see OnurByte/likee-linux notes) that nobody has run against a server. If Likee gives you a URL and key, store them like TikTok's and Lumen will push to them. Otherwise use the official **Likee Streamer** app for Windows. Lumen does not log in to Likee.

```bash
lumen-live accounts broadcast main-tt --ingest rtmps://<server from your platform> --video-kbps 4500
lumen-live accounts secret main-tt stream_key
lumen-live accounts authorize-broadcast main-tt --confirm   # you own the account / are authorized
lumen-live accounts broadcast-on main-tt
lumen-live scene set show main-tt main-kw
lumen-live broadcast --scene show --listen 127.0.0.1:9100
```

Check everything first, then test with a built-in card or a video file before using OBS (`--check` creates nothing and sends no video):

```bash
lumen-live broadcast --scene show --check
lumen-live broadcast --scene show --source test            # or --source /path/video.mp4
```

Point OBS at it with Settings → Output → Recording → Custom Output (FFmpeg), output to URL `tcp://127.0.0.1:9100`, container `mpegts`, or use any encoder that writes MPEG-TS to that address. Each destination re-encodes at its own bitrate in its own FFmpeg process with its own bounded queue. A destination that falls behind, is rejected or crashes restarts alone with backoff. Reading LIVE events never enables broadcasting. A destination is used only if it is enabled, explicitly authorized, and has both an ingest URL and an encrypted `stream_key`. Revoking authorization also turns broadcasting off.

### Logging in

The **Sign in** action on Studio's Connections page and `lumen-live accounts login REF [--browser PATH]` work the same for all three platforms. It opens the platform's own login page in a throwaway Chromium-family window, you log in there (password, code, QR and captcha stay with you), and Lumen stores the resulting session encrypted, then closes the window and deletes its profile.

| Platform | Page | Stored as | Used for |
| --- | --- | --- | --- |
| Kwai Global | `studio.kwai.com/user/login` | `studio_cookie` | Creating, starting and stopping Studio rooms |
| TikTok | `tiktok.com/login` | `cookie` | Authenticated LIVE reading |
| Likee Global | `likee.video` (Log in button) | `web_session` | `likee --account REF …`. It does not issue a stream key |

Sessions expire; `broadcast --check` reports it, and `accounts login` renews it. While the window is open, its DevTools port is reachable by other programs on this machine. Verified with a real browser against a local fake Studio (session captured, encrypted, profile deleted, on success, Ctrl-C and closed window) and against the real Kwai and Likee login pages without logging in. No real login has been run. The Kwai Studio bundle links no desktop installer, so Lumen targets the web Studio backend.

### Going LIVE on Kwai

1. **Get permission.** The account needs 500+ followers and approval: in the Kwai app, Camera → LIVE → Apply. Kwai replies with an in-app message. Without it, [Kwai's support page](https://www.kwai.com/support/live-permissions/i-want-to-request-permission-to-live-on-my-profile) is the route.
2. **Log in** at <https://studio.kwai.com/live/list> in your browser. If the page shows no way to create a LIVE, the account has no permission yet, and Lumen cannot change that.
3. **Pick one path:**
   - *Automatic (Lumen creates the room).* Log in once and set a cover:
     ```bash
     lumen-live accounts login main-kw
     lumen-live accounts set main-kw studio_cover /path/cover.jpg   # JPEG or PNG, up to 2 MB, 720×1280
     lumen-live accounts set main-kw studio_caption "Title"
     ```
   - *Manual (OBS-style).* Create a **Push-Stream** LIVE in Studio, copy its server and key, then store them with `accounts broadcast` and `accounts secret … stream_key`. Press **Start Live** in Studio once video is flowing, because Lumen only starts rooms it provisioned itself.
4. **Authorize and check:** `accounts authorize-broadcast main-kw --confirm`, `accounts broadcast-on main-kw`, `scene set show main-kw`, then `broadcast --scene show --check`. Expected: `ready main-kw: Kwai Studio session valid, LIVE permission granted…`.
5. **Go LIVE:** `broadcast --scene show --source test`. In automatic mode Lumen reuses any open room, starts the LIVE after about 512 KiB of video has been pushed, and ends it on Ctrl-C.

| `--check` says | Meaning | Fix |
| --- | --- | --- |
| session is missing or expired | The cookie is wrong, truncated or logged out | Log in again and store a fresh `studio_cookie` |
| no Kwai LIVE permission | The Studio session works but the account cannot go LIVE | Step 1 |
| set `studio_cover` / must be a JPEG or PNG | Kwai requires a cover image | Set a valid image |

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
