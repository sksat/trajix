---
name: run-scenario
description: |
  Run a trajix scenario for E2E playback testing. Connects to MCP Chrome via CDP,
  uploads a GNSS log file, and executes scenario-defined phases (speed changes, UI
  interactions, camera control). Use this to verify the app works end-to-end.
argument-hint: [--scenario NAME] [--log FILE]
---

# Run Scenario - E2E Playback Test

Execute a trajix scenario against a running dev server to verify end-to-end behavior.

## Prerequisites

1. **Dev server running**: `cd web && pnpm dev` (default: http://localhost:5173/trajix/)
2. **Playwright MCP Chrome running**: The MCP server must have a headed Chrome instance
3. **playwright-core installed**: `npm install --no-save playwright-core` in project root
4. **GNSS log file available**: e.g. `gnss_log_no_first_8min.txt` (607MB trimmed) or the full `gnss_log_2025_11_29_10_31_31.txt` (614MB)

## How to Run

```bash
# Basic run (default scenario: 2025-11-29-chitose-narita)
node scripts/run-scenario.mjs --log /path/to/gnss_log.txt

# Specify scenario
node scripts/run-scenario.mjs --scenario 2025-11-29-chitose-narita --log /path/to/gnss_log.txt

# Custom dev server URL
node scripts/run-scenario.mjs --log /path/to/gnss_log.txt --url http://localhost:4174/trajix/
```

## What It Does

1. Connects to existing MCP Chrome via CDP (auto-discovers port from process list)
2. Navigates to the dev server URL, detects stale state and reloads if needed
3. Optionally resizes window via i3wm (graceful skip on non-i3)
4. Uploads the log file via CDP `DOM.setFileInputFiles` (bypasses browser 50MB file chooser limit)
5. Waits for WASM parse completion (can take minutes for 600MB+ files)
6. Installs `clock.onTick` watcher for elapsed time polling
7. Runs the scenario's `run(ctx)` function with all helper methods
8. Reports completion

## Scenario File Format

Scenarios live in `scripts/scenarios/<name>.mjs`:

```javascript
export default {
  name: 'Human-readable Name',
  async run(ctx) {
    await ctx.followOn();
    await ctx.setSpeed(10);
    await ctx.play();
    await ctx.waitUntilElapsed(660);
    // ...
  },
};
```

### Available ctx Helpers

| Helper | Description |
|---|---|
| `sleep(ms)` | Wait for ms |
| `log(msg)` | Timestamped console log |
| `setSpeed(n)` | Change playback speed (1, 10, 50, 100, 500) via `<select>` |
| `waitUntilElapsed(sec)` | Poll sim time; auto-resumes on CLAMPED stopTime |
| `getClockState()` | Returns `{ elapsed, animating, speed }` |
| `play()` | Start playback (no-op if already playing) |
| `pause()` | Pause playback (no-op if already paused) |
| `followOn()` | Enable camera follow mode |
| `animatePitch(deg, ms)` | Smooth camera pitch animation (easeInOut) |
| `openDrawer()` | Open mobile sidebar drawer |
| `closeDrawer()` | Close mobile sidebar drawer |
| `scrollSidebarToBottom()` | Scroll sidebar to show charts |

## Troubleshooting

### "Cannot find Chrome CDP port"
Playwright MCP Chrome is not running. Start it first.

### "__cesiumViewer not found"
The viewer hasn't initialized yet. The script retries 10 times (1s each).
In bundled builds, `window.Cesium` doesn't exist -- the script uses
`viewer.clock.currentTime.constructor` to get JulianDate instead.

### Parse takes too long
614MB files take 2-5 minutes to parse. The script has a 10-minute timeout for parse.

### Playback auto-stops unexpectedly
Cesium clock reaches `stopTime` and sets `shouldAnimate = false` (CLAMPED behavior).
`waitUntilElapsed()` auto-detects this and resumes by clicking the play button.

### Speed change doesn't work
Must use `page.selectOption()`, NOT `dispatchEvent('change')`.
React controlled components only respond to Playwright's selectOption.

## Key Files

- Runner: `scripts/run-scenario.mjs`
- Scenarios: `scripts/scenarios/*.mjs`
- PlaybackControls: `web/src/components/PlaybackControls/PlaybackControls.tsx`
- CesiumMap: `web/src/components/CesiumMap/CesiumMap.tsx` (`window.__cesiumViewer`)