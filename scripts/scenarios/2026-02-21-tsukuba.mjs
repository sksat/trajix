/**
 * Scenario: Mt. Tsukuba hiking (2026-02-21)
 *
 * Log file: gnss_log_2026_02_21_11_42_28.txt (1.2GB)
 * Two segments: ascent and descent, merged into one GIF later.
 *
 * Segment 1 (ascent): elapsed 1200s → 6500s (20min to summit)
 *   Camera: heading 45°, pitch -15° (look up mountain from SW)
 *   Recording starts after 100x is set (skip 1x warmup)
 * Segment 2 (descent): elapsed 16800s → 20500s (ropeway to drive)
 *   Camera: heading 270°, pitch -25° (overlook Kanto Plain from summit)
 */
export default {
  name: 'Mt. Tsukuba Hiking',
  segments: true,

  async run(ctx) {
    // ════════════════════════════════════════════════════════════════
    // SEGMENT 1: Ascent (登山)
    // ════════════════════════════════════════════════════════════════

    // Seek to 20min mark (~1200s, on the trail ~350m)
    ctx.log('Segment 1: Ascent — seeking to elapsed 1200s');
    await ctx.seekTo(1200);

    // Camera setup (before recording)
    await ctx.followOn();
    await ctx.setHeading(45);
    await ctx.animatePitch(-15, 2000);
    await ctx.sleep(3000);

    // Start playback at 100x, then start recording
    await ctx.setSpeed(100);
    await ctx.play();
    await ctx.sleep(1000);  // Let camera stabilize at 100x

    await ctx.startRecording('ascent');

    // 100x ascent to summit
    ctx.log('  100x → summit (6500s)');
    await ctx.waitUntilElapsed(6500);

    // Pause at summit and end segment
    await ctx.pause();
    await ctx.sleep(3000);
    await ctx.stopRecording();

    // ════════════════════════════════════════════════════════════════
    // SEGMENT 2: Bus ride home (バス帰宅)
    // ════════════════════════════════════════════════════════════════

    // Seek to bus departure (~19100s, leaving parking lot)
    ctx.log('Segment 2: Bus ride — seeking to elapsed 19100s');
    await ctx.seekTo(19100);
    await ctx.sleep(3000);

    // Camera: overlooking plains from mountain (bus descends into plains)
    await ctx.setHeading(270);
    await ctx.animatePitch(-25, 2000);
    await ctx.sleep(2000);

    // Start at 100x, then record
    await ctx.setSpeed(100);
    await ctx.play();
    await ctx.sleep(1000);

    await ctx.startRecording('descent');

    // 100x bus ride through mountain roads and plains
    ctx.log('  100x → 21000s (bus ride home)');
    await ctx.waitUntilElapsed(21000);

    // Pause and end
    await ctx.pause();
    await ctx.sleep(3000);
    await ctx.stopRecording();
  },
};
