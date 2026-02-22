---
name: record-demo
description: |
  Record a demo video of trajix using ffmpeg x11grab + scenario playback.
  Manages the full take-iteration workflow: record, review, adjust, re-record.
  Use when the user wants to create demo GIFs/videos for README or documentation.
argument-hint: [--scenario NAME] [--take N] [--log FILE]
---

# Record Demo - Demo Video Recording with Take Iteration

Record demo videos of the trajix web app using ffmpeg screen capture + automated scenario playback.

## Prerequisites

1. **Everything from `run-scenario` skill** (dev server, Playwright MCP Chrome, playwright-core)
2. **ffmpeg installed** (x11grab support required -- Linux/X11 only)
3. **X11 display** (Wayland not supported by x11grab)
4. **i3wm** (optional -- for auto window resize/position detection; use `--capture` on other WMs)

## How to Record

```bash
# Record with auto i3 detection
node scripts/run-scenario.mjs --log /path/to/gnss_log.txt --record --take 1

# Record with manual capture area (non-i3 environments)
node scripts/run-scenario.mjs --log /path/to/gnss_log.txt --record --take 1 --capture 2836,525,500x896

# Record + GIF conversion
node scripts/run-scenario.mjs --log /path/to/gnss_log.txt --record --take 1 --gif
```

## Take Iteration Workflow

**A good demo is never captured in one shot.** Use the take iteration workflow:

1. **Record**: `node scripts/run-scenario.mjs --log FILE --record --take N`
2. **Review**: Play back `demo-rawN.mp4` in mpv/VLC, check timing/camera/UI
3. **Identify issues**: Camera angle wrong? Speed too fast? Drawer timing off?
4. **Fix**: Adjust scenario file or app code as needed
5. **Re-record**: `--take N+1` to keep both versions for comparison
6. **Finalize**: When satisfied, run with `--gif` for README-ready output

### Phase Target Determination

The elapsed seconds for each phase transition should be:
1. **Initial estimate**: Calculate from flight timeline + file startTime
2. **User override**: User watches the recording and says "switch speed at this point" -- **user values always win**
3. **Iterative refinement**: Fine-tune across takes based on visual review

### Common Issues Across Takes (Hard-Won Knowledge from 26+ Takes)

| Issue | Cause | Fix |
|---|---|---|
| Video is black/wrong area | Capture coordinates wrong | Use `--capture X,Y,WxH` or check i3 window detection |
| Speed doesn't change | `dispatchEvent('change')` doesn't fire React | Already fixed: uses `page.selectOption()` which fires React's onChange |
| Playback stops mid-recording | Cesium CLAMPED stopTime reached | `waitUntilElapsed()` auto-resumes; check targets don't exceed data range |
| Camera zoomed way out | Follow ON + 500x speed | 500x makes visibility-range zoom out to Japan-wide; use 100x max for close views |
| Stale state from previous parse | Browser has old data | Runner auto-detects `.play-btn` on fresh nav and reloads |
| elapsed offset wrong | Using full data file (614MB) | Use trimmed file (`gnss_log_no_first_8min.txt`) -- first 8min removed, startTime ~10:39 |
| MP4 unplayable (0 bytes) | moov atom not written on SIGINT | Already fixed: records MKV first (no moov atom needed), converts to MP4 after |
| Wrong Chrome window detected | Multiple Chrome windows open | i3 detection matches by window title containing "trajix" |
| `outerHeight - innerHeight` wrong | 13px error on i3wm | Measured values hardcoded: title=22px, border=2px, Chrome UI=87px |
| `window.Cesium` not found | Bundled build (Vite) has no global | Use `viewer.clock.currentTime.constructor` for JulianDate |
| Viewport doesn't match window | i3 resize doesn't update Chrome viewport | Use CDP `Emulation.setDeviceMetricsOverride` (`setViewportSize` fails over CDP) |
| `__animateFollowPitch` not found | Follow useEffect not run yet | `animatePitch()` auto-waits; ensure `followOn()` called first |

### Camera Behavior at Different Speeds

| Speed | Camera Behavior | Best For |
|---|---|---|
| 1x | Close-up ground view, smooth follow | Initial ground/taxi scenes |
| 10x | Detailed view, terrain visible | Takeoff, approach, landing |
| 50x | Medium zoom, descent visible | Descent phase |
| 100x | Wide view, route visible | Cruise phase |
| 500x | Japan-wide zoom out (not useful) | Route overview only (avoid with Follow ON) |

### Camera Pitch Presets (Tested on Flight Data)

- `-15deg`: Near-horizontal, sky visible -- **best for flight demos**
- `-20deg`: Horizon visible, quite lateral
- `-30deg`: Balanced terrain + altitude view
- `-56deg` (default): Looking down, good for ground detail
- Heading: Do NOT fix heading -- auto-tracking is more natural than fixed offset

Use `ctx.animatePitch(deg, ms)` for smooth transitions. Uses easeInOut curve,
auto-releases after 2 settle frames (behaves like user drag, not permanent override).

## Recording Technical Details

### ffmpeg Settings
- Format: x11grab (X11 screen capture)
- Codec: H.264 ultrafast (**only** codec fast enough for realtime 25fps; VP9 realtime = ~2fps = NG)
- Container: MKV during recording (resilient to incomplete writes), converted to MP4 after
- Frame rate: 25fps
- CRF: 23 (good quality, reasonable size)
- Typical output: ~100MB for 4-minute recording at 500x896

### i3wm Window Detection
- Title match: looks for "trajix" in Chrome window title via `i3-msg -t get_tree`
- Decoration offsets (measured): title=22px, border=2px
- Chrome UI (measured): 87px (tabs + address bar)
- **`outerHeight - innerHeight` returns 100px but real value is 87px** -- always use measured constants
- Viewport start: `rect.y + title + chrome_ui`, viewport size: `innerWidth x innerHeight`

### Mobile Viewport for Demo
- 720x1280 viewport (HD portrait) triggers mobile layout (`isMobile` = height > width)
- Viewport set via CDP `Emulation.setDeviceMetricsOverride` — `page.setViewportSize()` does NOT work over `connectOverCDP`
- i3 window resized to fit: adds title=22px, border=2x2px, Chrome UI=87px
- For final output, scale down with ffmpeg if needed (e.g. `scale=500:-1`)
- **Don't rely on i3 resize alone** — Chrome viewport does not follow window size changes

### File Upload via CDP
Browser file chooser has a ~50MB limit. CDP `DOM.setFileInputFiles` bypasses this
by passing a local file path directly to the browser process. Steps:
1. Create hidden `<input type="file">` via `page.evaluate`
2. Set file via `cdp.send('DOM.setFileInputFiles', { nodeId, files: [path] })`
3. Dispatch `DragEvent('drop')` on `.drop-zone` with the file from the input
4. Remove hidden input

### Parse Timing
- 607MB trimmed file: ~2-5 minutes to parse (WASM in Web Worker)
- 614MB full file: ~3-6 minutes; adds 8min offset to elapsed time (use trimmed)
- Parse completion: wait for `.play-btn` selector (appears when parse finishes)

### CLAMPED StopTime Behavior
Cesium clock reaches `stopTime` and auto-stops (`shouldAnimate = false`).
For the 11/29 log, stop happens at elapsed ~01:36:27.
`waitUntilElapsed()` detects `!animating && elapsed > 0` and clicks `.play-btn` to resume.
Scenario targets should not exceed the data range (e.g. max ~5800s for this flight).

### GIF Conversion Settings
```bash
ffmpeg -i demo-rawN.mp4 \
  -vf "fps=10,scale=500:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=bayer" \
  -loop 0 demo.gif
```
Target: <15MB for GitHub README. Reduce `max_colors` to 64 or fps to 5 if too large.

### Review Shortcuts
```bash
# Extract frame at timestamp T
ffmpeg -ss 30 -i demo-rawN.mp4 -frames:v 1 -y preview.png

# Quick preview of multiple timestamps
for t in 5 30 60 120 180; do
  ffmpeg -ss $t -i demo-rawN.mp4 -frames:v 1 -y "preview_${t}s.png"
done
```

## Scenario Segment Design Tips

### Chitose-Narita Flight (Reference Scenario)

Timeline (trimmed file, startTime ~10:39):
| elapsed | JST | Event |
|---|---|---|
| 0:00 | 10:39:32 | File start (ground) |
| ~1228s (20:28) | ~11:00 | Takeoff |
| ~3868s (1:04:28) | ~11:44 | Cruise 4650m, 644km/h |
| ~5188s (1:26:28) | ~12:06 | Final approach start |
| ~5379s (1:29:39) | 12:09:11 | Touchdown RWY34L |
| ~5428s (1:30:28) | ~12:10 | Taxi complete |

Adopted phase structure (user-approved):
1. Follow ON + pitch -15deg
2. 1x for 15s (camera convergence)
3. 10x to 660s (ground to takeoff)
4. Drawer open/scroll/close (show sky plot + charts)
5. 100x to 3600s (cruise skip)
6. 50x to 5100s (descent)
7. 10x to 5760s (approach + landing)
8. Pause + 5s hold

## Output Files

| File | Description |
|---|---|
| `demo-rawN.mkv` | Raw recording (MKV, always created) |
| `demo-rawN.mp4` | Converted MP4 (faststart, always created after recording) |
| `demo.gif` | GIF for README (only with `--gif`) |

## Key Files

- Runner: `scripts/run-scenario.mjs`
- Scenarios: `scripts/scenarios/*.mjs`
- PlaybackControls: `web/src/components/PlaybackControls/PlaybackControls.tsx` (camera pitch/heading API)
- CesiumMap: `web/src/components/CesiumMap/CesiumMap.tsx` (`window.__cesiumViewer` exposed here)
- App: `web/src/App.tsx` (mobile drawer, sidebar layout)
- Mobile breakpoint: `web/src/layout/sidebarState.ts` (768px)
- UI selectors: `.play-btn`, `.playback-speed`, `.follow-btn`, `.playback-seek`, `.playback-time`, `.sidebar-toggle`, `.sidebar-content`, `.drop-zone`