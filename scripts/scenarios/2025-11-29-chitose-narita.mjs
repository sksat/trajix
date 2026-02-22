/**
 * Scenario: Chitose → Narita flight (2025-11-29)
 *
 * Log file: gnss_log_no_first_8min.txt (607MB, trimmed — first 8min ground wait removed)
 * Total sim time: ~01:41, flight portion only
 * Timeline: ground → takeoff (~11:00) → cruise → descent → touchdown RWY34L (12:09:11) → taxi
 */
export default {
  name: 'Chitose → Narita Flight',

  async run(ctx) {
    // ── Phase 0: Follow ON + camera angle ──
    ctx.log('Phase 0: Follow ON + camera pitch');
    await ctx.followOn();
    await ctx.animatePitch(-15, 2000);
    await ctx.sleep(3000);

    // ── Phase 1: 1x ground start (15s) ──
    ctx.log('Phase 1: 1x ground (15s)');
    await ctx.setSpeed(1);
    await ctx.play();
    await ctx.sleep(15000);

    // ── Phase 2: 10x until sim 660s (11:00) ──
    ctx.log('Phase 2: 10x → sim 660s (11:00)');
    await ctx.setSpeed(10);
    await ctx.waitUntilElapsed(660);

    // ── Phase 3: Open drawer ──
    ctx.log('Phase 3: Open drawer');
    await ctx.openDrawer();
    await ctx.sleep(3000);

    // ── Phase 3b: Scroll down to see charts ──
    ctx.log('Phase 3b: Scroll to charts');
    await ctx.scrollSidebarToBottom();
    await ctx.sleep(3000);

    // ── Phase 3c: View charts ──
    ctx.log('Phase 3c: Viewing charts');
    await ctx.sleep(3000);

    // ── Phase 4: Close drawer ──
    ctx.log('Phase 4: Close drawer');
    await ctx.closeDrawer();
    await ctx.sleep(3000);

    // ── Phase 5: 100x cruise skip ──
    ctx.log('Phase 5: 100x → sim 3600s (01:00:00)');
    await ctx.setSpeed(100);
    await ctx.waitUntilElapsed(3600);

    // ── Phase 6: 50x descent ──
    ctx.log('Phase 6: 50x → sim 5100s (01:25:00)');
    await ctx.setSpeed(50);
    await ctx.waitUntilElapsed(5100);

    // ── Phase 7: 10x approach → landing ──
    ctx.log('Phase 7: 10x → sim 5760s (01:36:00)');
    await ctx.setSpeed(10);
    await ctx.waitUntilElapsed(5760);

    // ── Phase 8: Pause and end ──
    ctx.log('Phase 8: Pause');
    await ctx.pause();
    await ctx.sleep(5000);
  },
};
