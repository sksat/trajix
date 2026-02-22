#!/usr/bin/env node
/**
 * Scenario runner for trajix.
 * Connects to existing MCP Chrome via CDP, uploads a log file, and runs
 * a scenario (playback phases with speed changes, UI operations, etc.).
 *
 * By default runs the scenario without recording (E2E test mode).
 * With --record, captures the viewport via ffmpeg x11grab.
 *
 * Usage:
 *   node scripts/run-scenario.mjs --log FILE [--scenario NAME] [--url URL]
 *   node scripts/run-scenario.mjs --log FILE --record [--take N] [--gif] [--capture X,Y,WxH]
 *
 * Options:
 *   --log FILE        Path to GNSS log file (required)
 *   --scenario NAME   Scenario name (default: 2025-11-29-chitose-narita)
 *   --url URL         Dev server URL (default: http://localhost:5173/trajix/)
 *   --record          Enable ffmpeg x11grab recording
 *   --take N          Take number for output filenames (default: 1)
 *   --gif             Convert recording to GIF after MP4
 *   --capture X,Y,WxH Manual capture area (skip i3 auto-detection)
 *
 * Prerequisites:
 *   - MCP Chrome running (Playwright MCP server)
 *   - Dev server running (pnpm dev)
 *   - playwright-core installed (npm install --no-save playwright-core)
 *   - ffmpeg installed (only for --record)
 *
 * Recording design notes (rejected alternatives):
 *   - Playwright recordVideo: captures full OS window, not viewport. i3wm tiles
 *     the window huge, so the viewport ends up as a small region in a corner.
 *   - Screenshot loop (browser_take_screenshot): ~5fps max, animations too choppy.
 *   - CDP Page.startScreencast: ~30fps but runs in browser sandbox where
 *     require('fs') is unavailable, so frames can't be saved to disk.
 *   - VP9 realtime encoding (libvpx-vp9 -deadline realtime): 0.06x speed (~2fps
 *     effective), unusable for screen capture. H.264 ultrafast achieves ~0.85x.
 *   - MKV container is used during recording (resilient to incomplete writes /
 *     missing moov atom on SIGINT), then converted to MP4 afterwards.
 *
 * UI interaction notes:
 *   - Speed change MUST use page.selectOption(), not dispatchEvent('change').
 *     React controlled components ignore synthetic DOM events.
 *   - Camera pitch override uses frame-counter hold (SETTLE_FRAMES=2) then
 *     auto-releases. Boolean override locks camera permanently — rejected.
 *   - window.Cesium doesn't exist in Vite bundled builds. Access JulianDate
 *     via viewer.clock.currentTime.constructor instead.
 */

import { chromium } from 'playwright-core';
import { spawn, execSync } from 'child_process';
import { pathToFileURL, fileURLToPath } from 'url';
import path from 'path';

// ── Configuration ────────────────────────────────────────────────────

const DEFAULT_URL = 'http://localhost:5173/trajix/';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROJECT_DIR = path.resolve(__dirname, '..');

// Parse CLI args
const args = process.argv.slice(2);
const scenarioName = args.includes('--scenario')
  ? args[args.indexOf('--scenario') + 1]
  : '2025-11-29-chitose-narita';
const logFile = args.includes('--log')
  ? path.resolve(args[args.indexOf('--log') + 1])
  : null;
const devUrl = args.includes('--url')
  ? args[args.indexOf('--url') + 1]
  : DEFAULT_URL;
const doRecord = args.includes('--record');
const takeNum = args.includes('--take')
  ? Number(args[args.indexOf('--take') + 1])
  : 1;
const doGif = args.includes('--gif');
const manualCapture = args.includes('--capture')
  ? args[args.indexOf('--capture') + 1]
  : null;

// Output paths (only used when recording)
const OUTPUT_MKV = `${PROJECT_DIR}/demo-raw${takeNum}.mkv`;
const OUTPUT_MP4 = `${PROJECT_DIR}/demo-raw${takeNum}.mp4`;
const OUTPUT_GIF = `${PROJECT_DIR}/demo.gif`;

// ── Helpers ──────────────────────────────────────────────────────────

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function log(msg) {
  const ts = new Date().toISOString().slice(11, 19);
  console.log(`[${ts}] ${msg}`);
}

// ── i3wm helpers (optional — graceful fallback if i3 unavailable) ───

/**
 * Check if i3wm is available.
 */
function hasI3() {
  try {
    execSync('i3-msg -t get_version', { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

/**
 * Find Chrome window rect from i3wm tree.
 * Matches by page title (e.g. "trajix") to avoid picking up other Chrome windows.
 */
function i3FindChromeRect(titleMatch = 'trajix') {
  const tree = JSON.parse(execSync('i3-msg -t get_tree').toString());
  let found = null;
  function walk(node) {
    if (
      node.window_properties?.class === 'Google-chrome' &&
      node.name?.toLowerCase().includes(titleMatch.toLowerCase()) &&
      node.rect?.width > 0
    ) {
      found = node.rect;
    }
    for (const child of [
      ...(node.nodes || []),
      ...(node.floating_nodes || []),
    ]) {
      walk(child);
    }
  }
  walk(tree);
  return found;
}

/**
 * Resize Chrome window via i3 to tightly fit the viewport.
 * No-op if i3 is unavailable.
 */
async function i3ResizeWindow(page) {
  if (!hasI3()) {
    log('i3wm not detected, skipping window resize.');
    return;
  }

  // Target mobile viewport (HD portrait — triggers mobile layout via height > width)
  const TARGET_VP_W = 720;
  const TARGET_VP_H = 1280;

  // i3wm decoration offsets (measured values)
  const I3_TITLE = 22;
  const I3_BORDER = 2;
  const CHROME_UI = 87; // tabs + address bar height

  // 1. Set viewport via CDP (page.setViewportSize doesn't work over connectOverCDP)
  const cdpResize = await page.context().newCDPSession(page);
  await cdpResize.send('Emulation.setDeviceMetricsOverride', {
    width: TARGET_VP_W,
    height: TARGET_VP_H,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await sleep(500);

  // 2. Resize i3 window to fit the viewport
  const targetW = TARGET_VP_W + 2 * I3_BORDER;
  const targetH = TARGET_VP_H + I3_TITLE + CHROME_UI + 15;
  log(`Resizing Chrome window to ${targetW}x${targetH} via i3 (target viewport ${TARGET_VP_W}x${TARGET_VP_H})...`);
  try {
    execSync(
      `i3-msg '[class="Google-chrome"] floating enable, resize set ${targetW} ${targetH}'`,
    );
    await sleep(1000);
  } catch (e) {
    log(`WARNING: i3 resize failed: ${e.message}`);
  }

  const vp = await page.evaluate(() => ({
    w: window.innerWidth,
    h: window.innerHeight,
  }));
  log(`Viewport after resize: ${vp.w}x${vp.h}`);
  if (Math.abs(vp.w - TARGET_VP_W) > 4 || Math.abs(vp.h - TARGET_VP_H) > 4) {
    log(`WARNING: viewport ${vp.w}x${vp.h} does not match target ${TARGET_VP_W}x${TARGET_VP_H}`);
  }
}

/**
 * Auto-detect capture area from i3 window position.
 * Returns { x, y, w, h } for ffmpeg x11grab.
 */
async function i3DetectCaptureArea(page) {
  // i3wm decoration offsets
  const I3_TITLE = 22;
  const I3_BORDER = 2;
  const CHROME_UI = 87;

  const vp = await page.evaluate(() => ({
    w: window.innerWidth,
    h: window.innerHeight,
  }));

  const rect = i3FindChromeRect();
  if (!rect) {
    throw new Error(
      'Cannot auto-detect capture area: Chrome window not found in i3 tree. ' +
      'Use --capture X,Y,WxH to specify manually.',
    );
  }

  const capture = {
    x: rect.x + I3_BORDER,
    y: rect.y + I3_TITLE + CHROME_UI,
    w: vp.w,
    h: vp.h,
  };

  log(`i3 rect: x=${rect.x} y=${rect.y} w=${rect.width} h=${rect.height}`);
  log(`Capture: x=${capture.x} y=${capture.y} w=${capture.w} h=${capture.h}`);

  const contentW = rect.width - 2 * I3_BORDER;
  if (Math.abs(contentW - vp.w) > 4) {
    log(`WARNING: width mismatch — i3 content ${contentW} vs viewport ${vp.w}`);
  }

  return capture;
}

/**
 * Parse --capture X,Y,WxH string into { x, y, w, h }.
 */
function parseCaptureArg(str) {
  const m = str.match(/^(\d+),(\d+),(\d+)x(\d+)$/);
  if (!m) {
    throw new Error(`Invalid --capture format: "${str}". Expected X,Y,WxH (e.g. 3170,500,500x896)`);
  }
  return { x: Number(m[1]), y: Number(m[2]), w: Number(m[3]), h: Number(m[4]) };
}

// ── Playback helpers ─────────────────────────────────────────────────

/**
 * Discover CDP port from running Chrome processes.
 */
function findCdpPort() {
  try {
    const out = execSync(
      "ps aux | grep -oP '\\-\\-remote-debugging-port=\\K[0-9]+' | head -1",
    )
      .toString()
      .trim();
    return Number(out);
  } catch {
    throw new Error('Cannot find Chrome CDP port. Is Playwright MCP running?');
  }
}

/**
 * Install clock.onTick watcher in the browser.
 */
async function installClockWatcher(page) {
  // Retry until __cesiumViewer is available (may not be set immediately after parse)
  for (let attempt = 0; attempt < 10; attempt++) {
    const ok = await page.evaluate(() => !!window.__cesiumViewer);
    if (ok) break;
    log(`  waiting for __cesiumViewer... (attempt ${attempt + 1})`);
    await sleep(1000);
  }

  await page.evaluate(() => {
    const viewer = window.__cesiumViewer;
    if (!viewer) throw new Error('__cesiumViewer not found');

    // Get JulianDate from the viewer's clock instance (works in bundled builds
    // where window.Cesium may not exist)
    const JulianDate = viewer.clock.currentTime.constructor;

    window.__clockState = { elapsed: 0, animating: false, speed: 1 };

    viewer.clock.onTick.addEventListener((clock) => {
      const elapsed = JulianDate.secondsDifference(
        clock.currentTime,
        clock.startTime,
      );
      window.__clockState = {
        elapsed: Math.floor(elapsed),
        animating: clock.shouldAnimate,
        speed: clock.multiplier,
      };
    });
  });
  log('Clock watcher installed.');
}

/**
 * Read clock state from browser (via installed watcher).
 */
async function getClockState(page) {
  return page.evaluate(() => window.__clockState ?? { elapsed: 0, animating: false, speed: 1 });
}

/**
 * Poll until elapsed sim time reaches targetSec.
 * Detects CLAMPED stopTime and resumes playback.
 */
async function waitUntilElapsed(page, targetSec) {
  let lastLog = 0;
  while (true) {
    const { elapsed, animating } = await getClockState(page);
    if (elapsed >= targetSec) {
      log(`  elapsed ${elapsed}s >= target ${targetSec}s`);
      return true;
    }
    if (!animating && elapsed > 0) {
      log(`  stopTime reached at ${elapsed}s, resuming playback...`);
      await page.click('.play-btn');
      await sleep(500);
      continue;
    }
    const now = Date.now();
    if (now - lastLog > 30000) {
      log(`  waiting... elapsed ${elapsed}s / target ${targetSec}s`);
      lastLog = now;
    }
    await sleep(200);
  }
}

/**
 * Set playback speed via <select>.
 */
async function setSpeed(page, speed) {
  await page.selectOption('.playback-speed', String(speed));
  log(`  speed → ${speed}x`);
  await sleep(300);
}

/**
 * Wait for parse completion (progress bar appears then play button appears).
 */
async function waitForParse(page) {
  log('  waiting for progress bar...');
  await page.waitForSelector('.progress-container', { timeout: 30000 });
  log('  parse started');
  await page.waitForSelector('.play-btn', { timeout: 600000 });
}

/**
 * Build the scenario context object with all helper functions.
 */
function buildContext(page) {
  return {
    page,
    sleep,
    log,

    setSpeed: (speed) => setSpeed(page, speed),
    waitUntilElapsed: (sec) => waitUntilElapsed(page, sec),
    getClockState: () => getClockState(page),

    play: async () => {
      const { animating } = await getClockState(page);
      if (!animating) {
        await page.click('.play-btn');
        await sleep(300);
      }
    },
    pause: async () => {
      const { animating } = await getClockState(page);
      if (animating) {
        await page.click('.play-btn');
        await sleep(300);
      }
    },

    followOn: async () => {
      await page.click('.follow-btn');
      await sleep(500);
    },
    animatePitch: async (deg, ms = 2000) => {
      // Wait for Follow useEffect to register the function (needs isFollowing=true + frame)
      await page.waitForFunction(
        () => typeof window.__animateFollowPitch === 'function',
        { timeout: 10000 },
      );
      await page.evaluate(
        ({ deg, ms }) => window.__animateFollowPitch(deg, ms),
        { deg, ms },
      );
    },

    openDrawer: async () => {
      await page.click('.sidebar-toggle');
      await sleep(500);
    },
    closeDrawer: async () => {
      await page.click('.sidebar-toggle');
      await sleep(500);
    },
    scrollSidebarToBottom: async () => {
      const el = await page.$('.sidebar-content');
      if (el) {
        await el.evaluate((node) => {
          node.scrollTo({ top: node.scrollHeight, behavior: 'smooth' });
        });
      }
    },
  };
}

/**
 * Load scenario module from scripts/scenarios/<name>.mjs
 */
async function loadScenario(name) {
  const scenarioPath = path.resolve(PROJECT_DIR, 'scripts', 'scenarios', `${name}.mjs`);
  const mod = await import(pathToFileURL(scenarioPath).href);
  const scenario = mod.default;
  if (!scenario?.run) {
    throw new Error(`Invalid scenario "${name}": must export { name, run(ctx) }`);
  }
  return scenario;
}

// ── Recording helpers ────────────────────────────────────────────────

function startFfmpeg(capture) {
  log('Starting ffmpeg (MKV)...');
  const proc = spawn('ffmpeg', [
    '-f', 'x11grab',
    '-video_size', `${capture.w}x${capture.h}`,
    '-i', `:0+${capture.x},${capture.y}`,
    '-r', '25',
    '-c:v', 'libx264',
    '-preset', 'ultrafast',
    '-crf', '23',
    '-pix_fmt', 'yuv420p',
    '-y', OUTPUT_MKV,
  ]);
  proc.stderr.on('data', () => {});
  return proc;
}

async function stopFfmpeg(proc) {
  log('Stopping ffmpeg...');
  proc.kill('SIGINT');
  await new Promise((resolve) => {
    const timeout = setTimeout(() => {
      log('  ffmpeg timeout, sending SIGTERM...');
      proc.kill('SIGTERM');
    }, 5000);
    proc.on('close', () => {
      clearTimeout(timeout);
      resolve();
    });
  });
  log(`Recording saved: ${OUTPUT_MKV}`);

  const stat = execSync(`ls -lh "${OUTPUT_MKV}"`).toString().trim();
  log(`File: ${stat}`);
}

function convertToMp4() {
  log('Converting MKV → MP4...');
  execSync(
    `ffmpeg -i "${OUTPUT_MKV}" -c copy -movflags +faststart -y "${OUTPUT_MP4}"`,
    { stdio: 'inherit' },
  );
  const stat = execSync(`ls -lh "${OUTPUT_MP4}"`).toString().trim();
  log(`MP4: ${stat}`);
}

function convertToGif() {
  log('Converting to GIF...');
  execSync(
    `ffmpeg -i "${OUTPUT_MP4}" ` +
      '-vf "fps=10,scale=500:-1:flags=lanczos,' +
      'split[s0][s1];[s0]palettegen=max_colors=128[p];' +
      '[s1][p]paletteuse=dither=bayer" ' +
      `-loop 0 -y "${OUTPUT_GIF}"`,
    { stdio: 'inherit' },
  );
  const stat = execSync(`ls -lh "${OUTPUT_GIF}"`).toString().trim();
  log(`GIF: ${stat}`);
}

// ── Main ─────────────────────────────────────────────────────────────

async function main() {
  const scenario = await loadScenario(scenarioName);
  if (!logFile) {
    throw new Error('--log FILE is required');
  }
  const mode = doRecord ? `record (take ${takeNum})` : 'run';
  log(`=== ${scenario.name} [${mode}] ===`);
  log(`Log file: ${logFile}`);

  // 1. Connect to Chrome via CDP
  const cdpPort = findCdpPort();
  log(`Connecting to Chrome via CDP on port ${cdpPort}...`);
  const browser = await chromium.connectOverCDP(`http://localhost:${cdpPort}`);
  const contexts = browser.contexts();
  if (contexts.length === 0) throw new Error('No browser contexts found');
  const page = contexts[0].pages()[0];
  if (!page) throw new Error('No page found');
  log(`Connected. Page: ${page.url()}`);

  // 2. Navigate to fresh page
  log('Navigating to trajix...');
  await page.goto(devUrl, { waitUntil: 'networkidle' });
  await page.waitForSelector('.drop-zone', { timeout: 10000 });
  const hasPlayBtn = await page.$('.play-btn');
  if (hasPlayBtn) {
    log('WARNING: stale state, reloading...');
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForSelector('.drop-zone', { timeout: 10000 });
  }
  log('Drop zone visible (clean state).');

  // 3. Resize window (i3: ensures viewport fits; graceful skip otherwise)
  await i3ResizeWindow(page);

  // 4. Start ffmpeg if recording
  let ffmpeg = null;
  if (doRecord) {
    const capture = manualCapture
      ? parseCaptureArg(manualCapture)
      : await i3DetectCaptureArea(page);
    ffmpeg = startFfmpeg(capture);
    await sleep(2000);
  }

  // 5. Upload file via CDP
  log(`Uploading log file via CDP: ${logFile}`);
  const cdp = await page.context().newCDPSession(page);
  await page.evaluate(() => {
    const input = document.createElement('input');
    input.type = 'file';
    input.id = '__demo-upload';
    input.style.display = 'none';
    document.body.appendChild(input);
  });
  const { root } = await cdp.send('DOM.getDocument');
  const { nodeId } = await cdp.send('DOM.querySelector', {
    nodeId: root.nodeId,
    selector: '#__demo-upload',
  });
  await cdp.send('DOM.setFileInputFiles', { nodeId, files: [logFile] });
  await page.evaluate(() => {
    const input = document.getElementById('__demo-upload');
    const file = input.files[0];
    if (!file) throw new Error('No file in hidden input');
    const dropZone = document.querySelector('.drop-zone');
    const dt = new DataTransfer();
    dt.items.add(file);
    dropZone.dispatchEvent(
      new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }),
    );
    input.remove();
  });
  log('File upload triggered.');

  // 6. Wait for parse completion
  await waitForParse(page);
  log('Parse complete!');

  // Wait for map tiles + install clock watcher
  await sleep(5000);
  await installClockWatcher(page);

  const { elapsed: initElapsed } = await getClockState(page);
  log(`Initial elapsed: ${initElapsed}s`);

  await sleep(3000);
  log('Map tiles loaded.');

  // 7. Run scenario
  log(`Running scenario: ${scenario.name}`);
  const ctx = buildContext(page);
  await scenario.run(ctx);
  log('Scenario complete.');

  // 8. Stop recording + convert
  if (ffmpeg) {
    await stopFfmpeg(ffmpeg);
    convertToMp4();
    if (doGif) {
      convertToGif();
    }
  }

  log('=== Done ===');
  await browser.close();
}

main().catch((err) => {
  console.error('FATAL:', err);
  process.exit(1);
});
