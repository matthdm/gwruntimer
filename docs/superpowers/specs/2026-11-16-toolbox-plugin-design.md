# TIPS tracker as a GWToolbox++ plugin: design

Status: draft for approval, 2026-11-16.

## Intent and why

Runners already use GWToolbox++. They do not use our injected DLL and launcher, and the launcher gets flagged as malware.
The TIPS tracker needs to reach runners the way they already install things: drop one DLL into Toolbox's `plugins` folder,
tick it on. Toolbox itself is signed and trusted, and loading a plugin goes through Toolbox's normal route, so there is no
injector in our hands.

The plugin does what the tracker module in this repo does today (see `TIPS_TRACKER.md`), and nothing else. It is a native
Toolbox plugin: it uses Toolbox's plugin interface, ImGui context and GWCA, written in C++ like Toolbox, and must not
interfere with anything Toolbox does.

## Goals

1. **Feature parity with today's tracker** (the list below).
2. **Native to Toolbox.** Only the public plugin interface and GWCA's public API. Loads, unloads and "Refresh" cleanly.
3. **Honours the TIPS protocol exactly** (`docs/tracker-protocol.md` in the tips-run repo is the contract).
4. **Easy to install and hard to flag:** one DLL, no injector, no packer, no pattern scans or patching.

### Feature parity (what the plugin must do)

- **Token:** a password-style box, paste works, Save. Max 100 characters, trimmed. Stored in the plugin's settings and
  treated as a secret (it can post as the runner). Never logged.
- **Two modes**, chosen in the panel and fixed while tracking:
  - *Runner tracking (a run):* Refresh finds the next run (live, else earliest scheduled, else first queued) and shows its
    title, status and route name. **Start run** and **End run**.
  - *General tracking (no run):* **Start tracking** / **Stop tracking** against the presence endpoints. Starting is the
    consent, so position sharing is on for its duration.
- **Share my position on the map (x/y):** off by default. Only then are x/y, the map's coordinate bounds and the one-second
  trail samples sent.
- **Zone reporting:** on every map load while tracking, send `map_id` and `is_explorable` (and position and bounds if
  sharing). On Start, report the map the runner is already standing in.
- **Heartbeats:** every 30 s; every 5 s while sharing, with x/y and up to 20 `points`. When sharing is switched off, a bare
  heartbeat goes out straight away to clear the dot and trail. If sharing is switched on mid-zone, re-report the zone (at
  most 3 times per zone) so the server gets the bounds.
- **Status shown in the panel:** the message line, progress %, off-route flag, busy state, "Tracking" indicator.
- **Server outcomes:** `finished: true` ends the run and refreshes; `ignored: true` means another game client took over, so
  stop and say so; 401 clears nothing but tells the runner to make a new token; 404 and 409 return the panel to idle with
  the server's message; 429, 5xx and network errors retry with backoff (1 s, doubling), other 4xx are shown, not retried.
  A relaunch takes over its own old session with Start.
- **Tidy shutdown:** when Toolbox terminates the plugin or the game closes, best-effort end of the active run or presence.
- **Privacy and limits:** only `client_session` (random per load, 32 hex characters), `map_id`, `is_explorable`, and when
  sharing x/y, bounds and points. Never a character name, account, party, chat or memory contents. HTTPS only to a
  compiled-in host, no redirects followed, responses capped at 8 KB, only the documented fields read, 10 s timeout.
- **Never blocks a frame:** all network calls on a background thread, fed through a bounded queue; the game-facing code only
  touches a mutex and the queue.

### Differences from today's module (my calls)

- **No click-through/lock logic.** The old panel needed its own "Ctrl+Shift, Lock, Close" handling because hudhook owns
  the input. In Toolbox the window is a normal Toolbox window, shown or hidden the way Toolbox plugin windows are.
- **No position-source picker and no raw offset reads.** GWCA gives one canonical position (the controlled character's
  `x`, `y`, which are the same floats the old "vector B" read) and the map bounds (`MapContext::start_pos`/`end_pos`,
  which are the same four floats as the old read). The old picker existed because the offsets were uncertain. I keep a small
  collapsible "Position check" in the panel showing the current x/y and bounds so a runner can see they move.
- **Settings in Toolbox's settings folder** through the plugin's `LoadSettings`/`SaveSettings`, not `tips_tracker.cfg`.
  No migration of an old `tips_tracker.cfg`: the runner pastes the token again.

## Non-goals

- The timer, run log, categories, personal records, map-name tables, the launcher, or anything else from gwruntimer.
- Migrating the old config, or sharing code with the Rust tracker.
- Upstreaming into Toolbox itself (possible later; not designed for here).
- Removing or changing the Rust tracker in this repo. It stays as it is.
- Non-Windows builds.

## Anti-goals (a failure even if it technically works)

- Anything that conflicts with Toolbox: our own Direct3D hook, our own input or `WndProc` capture beyond what the plugin
  interface offers, chat commands that collide with Toolbox's, patching the game, scanning memory for offsets.
- Anything that stalls a frame or the game thread.
- Anything that looks like what malware does: injection, packing, self-modifying code, obscured strings, undocumented
  network endpoints.
- Sending more than the protocol allows, or anything while the runner has not pressed Start.

## Constraints

- **32-bit, MSVC v143+ (Visual Studio 2022 or later), CMake**, same as Toolbox. The plugin shares C++ types
  (`std::filesystem::path`, `ToolboxIni`, the `ToolboxPlugin` vtable) with Toolbox across the DLL boundary, so it must be
  built from the headers of the Toolbox release it will run on, with a matching toolset and runtime settings. Pin to a
  Toolbox release (currently 8.34) and rebuild when Toolbox's plugin interface changes.
- **Plugin interface:** export `ToolboxPlugin* ToolboxPluginInstance()`; derive from `ToolboxPlugin` (or `ToolboxUIPlugin`
  for a window); implement `Initialize`, `Update`, `Draw`, `DrawSettings`, `LoadSettings`, `SaveSettings`,
  `SignalTerminate`, `CanTerminate`, `Terminate`.
- **Everything that reads the game runs on Toolbox's game/render thread** (`Update`/`Draw`), never from the worker thread.
- **Host:** compiled in via a CMake option (`TIPS_API_BASE`). A debug build may use `http://localhost`; release is HTTPS only.
- The protocol is the contract; the Rust `uplink.rs` and its mock-server tests are the behavioural reference.

## Approaches considered

- **A. Pure C++ plugin, protocol logic ported from `uplink.rs`. (Chosen.)** One toolchain, same language as Toolbox, uses
  GWCA instead of hand-rolled offsets, and small. Cost: the protocol logic lives in two languages until the old tracker is retired.
- **B. Thin C++ plugin around the Rust uplink as a static library.** Reuses tested code, but needs both toolchains, links in
  `rustls` (larger, more likely to be flagged), and is not what was asked for ("native, same language as Toolbox").
- **C. Run the existing DLL next to Toolbox.** Rejected: both would hook Direct3D 9, and nothing improves distribution.
- **D. Upstream the tracker into Toolbox.** Possible later; depends on the Toolbox maintainers.

## Design

### Parts

1. **`tracker_core`** (no game and no ImGui dependencies; plain C++): `Config`, `Status`, the command queue, the worker thread
   and the protocol state machine ported from `Worker` in `uplink.rs`, plus the `Latest` sample buffer (positions, bounds, zone
   generation). It talks to the network through a small `Transport` interface (`get(path, token)`, `post(path, token, body)`
   returning status plus capped body). This is what makes it testable without the game.
2. **`RestClientTransport`**: the real transport, a small subclass of Toolbox's own `HttpRequest` (`RestClient/HttpClient.h` and
   `.cpp`, MIT licensed, a thin wrapper over WinHTTP). The plugin compiles those two files from the pinned Toolbox checkout into
   its own DLL (it cannot call Toolbox's copy at runtime). Checked against the Toolbox source in `reference_material/GWToolboxpp`
   (8.34): those two files depend only on the standard library and WinHTTP, not on Toolbox's `Core` library, which only the async
   client (`RestClient.cpp`) needs. We use the blocking `Perform()` from the plugin's own worker thread.
   - Already right by default: certificate and host checks on (`m_VerifyPeer`/`m_VerifyHost` default true, never turned off by
     us), redirects never followed (`m_FollowLocation` defaults false), `X-Tracker-Token`/JSON headers and POST bodies, timeouts.
   - We add: **HTTPS only** (the class allows plain HTTP, so the transport refuses any URL that is not `https://`, except a debug
     build to `http://localhost`); **our own User-Agent** (the default is "GWToolbox", which must not be used); the **8 KB response
     cap** (override `OnContent` to stop appending past the cap and treat overflow as an error); our 10 s timeout (the defaults are 30 s and 60 s).
   - Not used: its proxy default is "no proxy"; runners behind a mandatory proxy are out of scope.
3. **`TipsTrackerPlugin`** (the Toolbox plugin): the glue and the panel.
   - `Update`: poll `GW::Map::GetIsMapLoaded()` and `GetMapID()`. When a load finishes (or the plugin first sees a loaded map),
     call `push_zone(map_id, is_explorable, bounds)`. Polling instead of a game-event hook means nothing to register and
     nothing to clash with. Read the controlled character's x/y about four times a second while the panel is open or sharing is
     on, and hand it over. Bounds are read only while sharing.
   - `Draw`: the panel (token, mode, Refresh, Start/End, Share tick, status, position check).
   - `LoadSettings`/`SaveSettings`: token, share_position, tracking_mode.
   - `SignalTerminate`/`CanTerminate`/`Terminate`: stop the worker, send a best-effort end, return `CanTerminate() == false`
     until the worker has finished, so Toolbox can unload or refresh without leaving a thread running in unmapped code.
4. **Build**: a CMake project in `toolbox-plugin/` in this repo, building against a pinned Toolbox checkout for the plugin base,
   GWCA, ImGui headers and the two `RestClient` files. There is already a local clone at 8.34 in
   `reference_material/GWToolboxpp`, but that folder is git-ignored, so the repo itself pins Toolbox (git submodule at the 8.34
   tag, my call) so a fresh checkout builds the same. Produces `TipsTracker.dll` (name is my call).

### Panel (sketch)

```
TIPS Tracker                                      [window per Toolbox]
Tracker token (from My shop on the TIPS site)
[**********************************] [Save]
Mode:  (o) Runner tracking (a run)   ( ) General tracking (no run)

Next run: Beacon's -> Drok's  (scheduled)   route: Beacon's Perch to Droknar's Forge
[Refresh]  [Start run]            <- [End run] while a run is live
Tracking: sending your map to TIPS.        Progress 33%   (off route)
status message line

[ ] Share my position on the map (x/y)
> Position check:  x -1234.5  y 9876.2   bounds [-18432 -21504 21504 21504]
Sent to TIPS only while a run is active: map ID, town or explorable, and (if sharing) x/y.
```
In General mode the run row is replaced by a note that starting makes the position and zone trail public, with
**Start tracking** / **Stop tracking**.

### Testing

- **Core:** a unit-test executable (no game) driving `tracker_core` with a fake `Transport` and a controllable clock, porting
  the scenarios the Rust tests cover: Start sends `start` then the current `zone`; Start with an `ignored` reply; zone with
  progress and `finished`; heartbeat cadence 30 s and 5 s; points and sample caps; sharing switched off clears; bounds
  re-report capped at 3; 401, 404, 409 and 429/5xx behaviour; retry backoff; one-session takeover; General mode; nothing is
  sent before Start or after End; the token and position are never sent when they should not be.
- **Transport:** a test against a local mock HTTP server, plus the same optional end-to-end run against the local TIPS API
  (`npm run dev:lite`, the seeded token) that the Rust tests have.
- **In game (manual, by the runner):** load the plugin in Toolbox, Refresh from the plugin list, run through Start, a zone
  change, Share on and off, End, General mode, and quitting the game mid-run. I cannot run these.

### Other calls made while writing this (marked as mine)

- **Toolbox versions:** target the current Toolbox release only (8.34 now). Toolbox auto-updates its users, and the plugin shares C++
  types with Toolbox, so one build per Toolbox release is the honest model. The README says which Toolbox release a build is for.
- **Token box and paste:** the old module had to supply its own clipboard for imgui. Toolbox's ImGui context may already have one.
  The builder checks that Ctrl+V works in the token box inside Toolbox, and only if it does not, adds a minimal clipboard hook
  through the plugin interface (not a new global hook).
- **Release host:** the TIPS site URL is a build option; the release host is whatever you deploy to (the Rust build also has
  it as a placeholder today). It is needed to cut a release build, not to build or test the plugin.

### Left to the builder

Exact GWCA call names and the safest way to read the controlled character and map context; whether to derive from
`ToolboxUIPlugin`; how the pinned Toolbox checkout is built or consumed (headers only vs building `plugins/Base`); the JSON
approach (a header-only library already in Toolbox's dependencies if one is, else a small writer/parser for the few fields);
random bytes for the session id (`BCryptGenRandom` or similar); code layout; DLL name; whether a code-signing step is added
later.
