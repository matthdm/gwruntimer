# TIPS live tracking (this fork's addition)

> **Also available as a GWToolbox++ plugin** (no launcher, nothing injected): see [`toolbox-plugin/README.md`](toolbox-plugin/README.md).
> The text below describes the tracker module inside this timer.

Tells the [TIPS run board](https://github.com/matthdm/tips-run) which map the runner is in, and optionally where inside
it, so customers can follow a run. The existing timer is unchanged: the tracker does nothing until a token is entered and
**Start run** is pressed.

## Using it

1. On the TIPS site, open **My shop → Live tracking** and make a tracker token.
2. In game, hold **Ctrl+Shift**, right-click the timer and choose **TIPS Tracker**. The panel then stays interactive
   after you let go of the keys, so you can type and paste. Press **Lock (click-through)** to hand the mouse and
   keyboard back to the game (the panel stays on screen), or **Close** to hide it.
3. Paste the token and press **Save**. Your next run appears. Press **Start run** when you set off, **End run** when done.
   (The run also ends itself when the route's last map is reached.)
4. Optional: tick **Share my position on the map (x/y)**. Off by default.

The token is saved in `tips_tracker.cfg` next to the DLL. Treat it like a password: it can post as you.

## Two modes

- **Runner tracking (a run)**: what is described above. Tied to a run: Start run to begin, End run to stop.
- **General tracking (no run)**: pick it in the panel's mode choice. **Start tracking** makes your position and the trail
  of where you have been in the current zone public on the TIPS map until you press **Stop tracking**, with no run behind
  it. Use it to show an experimental route, or the way through a zone. Starting it is the consent, so the Share tick is
  not needed. It shows on the map page only (marked "exploring"), and a live run's dot replaces it while you are on a run.
  The mode cannot be changed while tracking.

A relaunched game takes over from its old session: press Start run (or Start tracking) again and the server hands it to
the new session instead of locking you out.

## What it sends

Only between Start run and End run, over HTTPS to the compiled-in host: the map ID, whether it's a town or explorable area,
a random session ID, and (if sharing is on) x/y inside the map. While sharing it also samples your position once a second and
sends the samples (at most 20) with the 5-second heartbeat as `points`, and sends the map's coordinate bounds with the zone
change, so the site can draw a dot and a trail of where you have been in the current zone (public; it clears when you enter the
next zone or the run ends). If you tick Share part-way through a zone, the zone is reported again so the site gets its bounds.
Never a name, account, party or chat.
Full contract: `docs/tracker-protocol.md` in the tips-run repo.

## Files

- `src/uplink.rs`: background thread, bounded channel, HTTPS client (ureq + rustls), retry with backoff. The game's render
  thread only touches a mutex and the channel, so the network can never stall a frame.
- `src/position.rs`: reads the character's x/y from the `AgentMovement` vectors at +0x64 and +0x74 (same layout: an unused
  float, then x, then y) and the map's coordinate bounds from the map context. Every read goes through `ReadProcessMemory`
  on our own process, which fails instead of crashing the game. The panel's *Position debug* section shows both vectors so you
  can see them move as you walk, and the **Position source** box picks which one is sent (both read the same on the client
  tested; +0x64 is the default).
- `src/clipboard.rs`: the Windows clipboard for imgui, so Ctrl+V pastes into the token box (imgui is built without its own).
- `src/lib.rs`: a few lines in the zone-change branch, a once-a-quarter-second position sample, and the panel.

## Building

```powershell
# Debug build, can talk to a local TIPS server (npm run dev:lite in the tips-run repo):
$env:TIPS_API_BASE = "http://localhost:4280"; cargo build

# Release build: HTTPS only, host fixed at compile time (set it to the deployed site):
$env:TIPS_API_BASE = "https://<your-site>.azurestaticapps.net"; cargo build --release
```

`cargo test` runs the uplink against a mock server. To check it against a real local TIPS server, with its sample data:

```powershell
$env:TIPS_E2E_BASE = "http://localhost:4280"
$env:TIPS_E2E_TOKEN = "tips_trk_dev-token-for-local-testing-only"
cargo test e2e -- --ignored --nocapture   # needs a scheduled run on the sample shop "Mule Bus"; also checks /api/live-trails gets the points in order
```
