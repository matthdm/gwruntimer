# TIPS Tracker for GWToolbox++

The TIPS live tracker as a **GWToolbox++ plugin**. It does what the tracker module in this repo's timer does
(see `../TIPS_TRACKER.md`): tells the [TIPS run board](https://tips-run.com) which map you are in and, only if you
switch it on, where inside it. It uses Toolbox's plugin interface and GWCA; there is no launcher and nothing is
injected. The timer is not part of it.

## Install

Requires **GWToolbox++ 8.34** (see "Build and test" below for why the version matters).

1. Download `TipsTracker.dll`.
2. Copy it into the `plugins` folder inside your Toolbox data folder. By default that is
   `%USERPROFILE%\Documents\GWToolboxpp\<a folder named after your computer>\plugins\`: open
   `Documents\GWToolboxpp`, go into the folder that contains `GWToolbox.ini`, and use (or create) `plugins` there.
3. Start Guild Wars and Toolbox. If the game is already running, that is fine: use **Refresh** in step 4.
4. In Toolbox open **Settings > Plugins**. Press **Refresh** if `TipsTracker.dll` is not listed, then press
   **Load** next to it. The *TIPS Tracker* window opens; the small checkbox on the same row shows or hides it.
   Toolbox remembers that the plugin is loaded and loads it again next time.

To remove it: press **Unload** in the same place, then delete `TipsTracker.dll` (and `TIPS Tracker.json`, which holds
your saved token, from the same folder).

Toolbox shows its own warning about third-party plugins: ArenaNet does not permit plugins, and they run with the
game's permissions. This DLL is not code-signed.

## If something goes wrong

- **Not listed under Plugins:** the DLL is in the wrong folder (it must be next to your `GWToolbox.ini`'s `plugins`
  folder, not the Toolbox program folder). Press **Refresh**.
- **"Failed to load plugin" in Toolbox's log:** Windows Defender or another antivirus may have blocked the DLL (Toolbox
  says so when it can tell), or your Toolbox is not 8.34. Allow the file, or update/downgrade Toolbox.
- **"TIPS rejected the token":** make a new token on the site and Save it again; a new token replaces the old one.
- **Nothing on the TIPS map:** check the status line in the window. General tracking needs **Start tracking**; a run
  needs **Start run**. Your position only shows while sharing is on (always on in General tracking).
- **Position check shows no x/y:** you are on a loading screen or have not entered a map yet.

## First use

1. On the TIPS site open **My shop > Live tracking** and make a tracker token.
2. Paste it into the token box and press **Save**. Treat it like a password: it can post as you.
3. **Runner tracking:** press **Refresh** to see your next run, then **Start run** when you set off and **End run**
   when done (the run also ends itself at the route's last map).
   **General tracking:** pick it, then **Start tracking** / **Stop tracking**. This makes your position and zone trail
   public on the TIPS map until you stop, with no run behind it.
4. Optional (runner tracking): tick **Share my position on the map (x/y)**. Off by default.
5. *Position check* in the panel shows the map, your x/y and the map bounds, so you can see they move as you walk.

Settings (token, share, mode) are saved by Toolbox in `plugins\TIPS Tracker.json`.

## What it sends

Only between Start and End, over HTTPS to `https://tips-run.com`: the map ID, whether it is a town or explorable, a
random session ID and, if sharing, x/y inside the map (plus a once-a-second trail while sharing). Never a name,
account, party or chat. The protocol is `docs/tracker-protocol.md` in the tips-run repo.

## Build and test

Needs Visual Studio 2026 (v18) with the C++ desktop workload, git, and the pinned Toolbox source: a clone of
`gwdevhub/GWToolboxpp` at the release in `TOOLBOX_VERSION` (8.34), by default in `../reference_material/GWToolboxpp`
(`-ToolboxDir` overrides it). The first run configures Toolbox's whole tree and fetches its vcpkg dependencies, which
takes a while.

```powershell
.\build.ps1 -Target tips_tests -Run                 # the unit tests (no game needed)
.\build.ps1 -Target TipsTracker -Dist               # builds dist\TipsTracker.dll
.\build.ps1 -Target TipsTracker -Dist -ApiBase https://example.net   # another host
```

How it builds: the plugin must be built inside Toolbox's CMake tree (it shares Toolbox's headers and settings types).
`build.ps1` makes a directory junction `<toolbox>/plugins/TipsTracker -> toolbox-plugin/plugin` and passes
`cmake/tips.cmake` to Toolbox's CMake, which adds our two targets at the end. No Toolbox file is edited.

**The DLL shares C++ types with Toolbox, so it must be built from the source of the Toolbox release it will run on.**
When Toolbox updates, update the clone and `TOOLBOX_VERSION`, rebuild, and re-test.

Debug builds (and `-DTIPS_ALLOW_LOCALHOST`) may talk to `http://localhost` for a local TIPS server; release builds are
HTTPS only. To check the transport against a local server (`npm run dev:lite` in the tips-run repo):

```powershell
$env:TIPS_E2E_BASE = "http://localhost:4280"
$env:TIPS_E2E_TOKEN = "tips_trk_dev-token-for-local-testing-only"
.\build.ps1 -Target tips_tests -Run     # live_check_against_a_local_tips_api_when_configured now runs
```

## Layout

- `core/tips/`: the game-free tracker (types, JSON, protocol `Engine`, threaded `Uplink`, `ZoneWatcher`). Unit-tested
  with a fake transport.
- `plugin/`: `TipsTrackerPlugin` (Toolbox glue and panel) and `RestClientTransport` (Toolbox's own `HttpRequest`).
- `tests/`: the tests. `docs/superpowers/specs/2026-11-16-toolbox-plugin-design.md` is the design.
