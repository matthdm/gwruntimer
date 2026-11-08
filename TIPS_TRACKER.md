# TIPS live tracking (this fork's addition)

Tells the [TIPS run board](https://github.com/matthdm/tips-run) which map the runner is in, and optionally where inside
it, so customers can follow a run. The existing timer is unchanged: the tracker does nothing until a token is entered and
**Start run** is pressed.

## Using it

1. On the TIPS site, open **My shop → Live tracking** and make a tracker token.
2. In game, hold **Ctrl+Shift**, right-click the timer and choose **TIPS Tracker**.
3. Paste the token and press **Save**. Your next run appears. Press **Start run** when you set off, **End run** when done.
   (The run also ends itself when the route's last map is reached.)
4. Optional: tick **Share my position on the map (x/y)**. Off by default.

The token is saved in `tips_tracker.cfg` next to the DLL. Treat it like a password: it can post as you.

## What it sends

Only between Start run and End run, over HTTPS to the compiled-in host: the map ID, whether it's a town or explorable area,
a random session ID, and (if sharing is on) x/y inside the map. Never a name, account, party or chat.
Full contract: `docs/tracker-protocol.md` in the tips-run repo.

## Files

- `src/uplink.rs`: background thread, bounded channel, HTTPS client (ureq + rustls), retry with backoff. The game's render
  thread only touches a mutex and the channel, so the network can never stall a frame.
- `src/position.rs`: reads the character's x/y. **Experimental**: offsets and the agent-array byte pattern come from GWCA
  and are unconfirmed against a running client. Every read goes through `ReadProcessMemory` on our own process, which fails
  instead of crashing the game. The panel's *Position debug* section shows every candidate so you can see which one moves
  as you walk, and the **Position source** box picks which one is sent.
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
cargo test e2e -- --ignored --nocapture   # needs a scheduled run on the sample shop "Mule Bus"
```
