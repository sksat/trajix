use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

use trajix::geo;
use trajix::prelude::*;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: trajix <gnss_log.txt>");
        std::process::exit(1);
    });

    let file = File::open(&path).unwrap_or_else(|e| {
        eprintln!("Failed to open {path}: {e}");
        std::process::exit(1);
    });
    let reader = BufReader::new(file);
    let mut parser = StreamingParser::new(reader);

    let mut fixes: Vec<FixRecord> = Vec::new();
    let mut record_counts: HashMap<&'static str, u64> = HashMap::new();
    let mut errors: u64 = 0;

    for result in &mut parser {
        match result {
            Ok(record) => {
                let label = match &record {
                    Record::Fix(_) => "Fix",
                    Record::Status(_) => "Status",
                    Record::Raw(_) => "Raw",
                    Record::UncalAccel(_) => "UncalAccel",
                    Record::UncalGyro(_) => "UncalGyro",
                    Record::UncalMag(_) => "UncalMag",
                    Record::OrientationDeg(_) => "OrientationDeg",
                    Record::GameRotationVector(_) => "GameRotationVector",
                    Record::Skipped => "Skipped",
                };
                *record_counts.entry(label).or_default() += 1;

                if let Record::Fix(fix) = record {
                    fixes.push(fix);
                }
            }
            Err(_) => errors += 1,
        }
    }

    // Header
    if let Some(h) = parser.header() {
        println!("=== Device ===");
        println!("  {} {} ({})", h.manufacturer, h.model, h.version);
        println!("  GNSS: {}", h.gnss_hardware);
        println!();
    }

    // Record counts
    println!("=== Record Counts ===");
    let mut counts: Vec<_> = record_counts.iter().collect();
    counts.sort_by(|a, b| b.1.cmp(a.1));
    for (label, count) in &counts {
        println!("  {label:20} {count:>10}");
    }
    if errors > 0 {
        println!("  {:20} {errors:>10}", "Errors");
    }
    println!("  {:20} {:>10}", "Total lines", parser.line_number());
    println!();

    // Fix analysis
    analyze_fixes(&fixes);

    // Altitude spike analysis
    println!("\n=== Altitude Analysis ===");
    analyze_altitude(&fixes);

    // Dead Reckoning pipeline analysis (second pass)
    println!("\n=== Dead Reckoning Pipeline ===");
    analyze_dr(&path);
}

fn analyze_fixes(fixes: &[FixRecord]) {
    if fixes.is_empty() {
        println!("No Fix records found.");
        return;
    }

    // Summary via library stats API
    let summary = trajix::stats::summarize_fixes(fixes);
    println!("=== Fix Summary ===");
    println!("  Count: {}", summary.count);
    println!(
        "  Duration: {:.0}s ({:.1} hours)",
        summary.duration_s,
        summary.duration_s / 3600.0
    );
    println!(
        "  Total distance: {:.0}m ({:.1} km)",
        summary.total_distance_m,
        summary.total_distance_m / 1000.0
    );
    if let Some(ref acc) = summary.accuracy {
        println!(
            "  Accuracy (m): min={:.1}, median={:.1}, p90={:.1}, p95={:.1}, max={:.1}",
            acc.min, acc.median, acc.p90, acc.p95, acc.max
        );
    }
    for pc in &summary.per_provider {
        println!("  {}: {} fixes", pc.provider.as_str(), pc.count);
    }
    println!();

    // Fix quality classification (matches web UI's FixQualitySummary)
    let qualities =
        trajix::quality::classify_fixes(fixes, trajix::quality::DEFAULT_GAP_THRESHOLD_MS);
    let mut primary = 0usize;
    let mut gap_fallback = 0usize;
    let mut rejected = 0usize;
    for q in &qualities {
        match q {
            trajix::quality::FixQuality::Primary => primary += 1,
            trajix::quality::FixQuality::GapFallback => gap_fallback += 1,
            trajix::quality::FixQuality::Rejected => rejected += 1,
        }
    }
    println!("=== Fix Quality Classification ===");
    println!("  Primary (GPS+FLP): {primary}");
    println!("  GapFallback (NLP during gap): {gap_fallback}");
    println!("  Rejected (NLP redundant): {rejected}");
    println!();

    // Per-provider stats
    let mut by_provider: HashMap<FixProvider, Vec<&FixRecord>> = HashMap::new();
    for f in fixes {
        by_provider.entry(f.provider).or_default().push(f);
    }

    println!("=== Fix Records ({} total) ===", fixes.len());
    for provider in [FixProvider::Gps, FixProvider::Flp, FixProvider::Nlp] {
        let Some(pf) = by_provider.get(&provider) else {
            continue;
        };
        println!("\n  --- {} ({} records) ---", provider.as_str(), pf.len());

        // Accuracy distribution
        let mut accuracies: Vec<f64> = pf.iter().filter_map(|f| f.accuracy_m).collect();
        accuracies.sort_by(|a, b| a.partial_cmp(b).unwrap());

        if !accuracies.is_empty() {
            let min = accuracies[0];
            let max = accuracies[accuracies.len() - 1];
            let median = accuracies[accuracies.len() / 2];
            let p90 = accuracies[(accuracies.len() as f64 * 0.9) as usize];
            let p95 = accuracies[(accuracies.len() as f64 * 0.95) as usize];
            let p99 = accuracies[(accuracies.len() as f64 * 0.99) as usize];

            println!(
                "    Accuracy (m): min={min:.1}, median={median:.1}, p90={p90:.1}, p95={p95:.1}, p99={p99:.1}, max={max:.1}"
            );

            // Histogram buckets
            let buckets = [5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0];
            print!("    Histogram: ");
            let mut prev = 0.0;
            for &b in &buckets {
                let count = accuracies.iter().filter(|&&a| a >= prev && a < b).count();
                if count > 0 {
                    print!("[{prev:.0}-{b:.0}m)={count} ");
                }
                prev = b;
            }
            let rest = accuracies.iter().filter(|&&a| a >= prev).count();
            if rest > 0 {
                print!("[{prev:.0}m+)={rest}");
            }
            println!();
        }

        // Missing fields
        let no_alt = pf.iter().filter(|f| f.altitude_m.is_none()).count();
        let no_speed = pf.iter().filter(|f| f.speed_mps.is_none()).count();
        let no_bearing = pf.iter().filter(|f| f.bearing_deg.is_none()).count();
        if no_alt > 0 || no_speed > 0 || no_bearing > 0 {
            println!("    Missing: alt={no_alt}, speed={no_speed}, bearing={no_bearing}");
        }
    }

    // Coverage gap analysis: where does NLP appear relative to GPS/FLP?
    println!("\n=== Coverage Gap Analysis ===");
    analyze_coverage_gaps(fixes);

    // Consecutive fix jump analysis (all providers mixed, sorted by time)
    println!("\n=== Consecutive Fix Jumps ===");
    analyze_jumps(fixes);

    // Per-provider jump analysis
    for provider in [FixProvider::Gps, FixProvider::Flp, FixProvider::Nlp] {
        if let Some(pf) = by_provider.get(&provider) {
            let mut sorted: Vec<&FixRecord> = pf.to_vec();
            sorted.sort_by_key(|f| f.unix_time_ms);
            println!("\n  --- Jumps within {} only ---", provider.as_str());
            analyze_jumps_ref(&sorted);
        }
    }
}

fn analyze_coverage_gaps(fixes: &[FixRecord]) {
    // Sort by time
    let mut sorted: Vec<&FixRecord> = fixes.iter().collect();
    sorted.sort_by_key(|f| f.unix_time_ms);

    if sorted.is_empty() {
        return;
    }

    let t_start = sorted[0].unix_time_ms;
    let t_end = sorted[sorted.len() - 1].unix_time_ms;
    let total_s = (t_end - t_start) as f64 / 1000.0;
    println!(
        "  Total time span: {:.0}s ({:.1} hours)",
        total_s,
        total_s / 3600.0
    );

    // Find GPS/FLP coverage gaps (periods where no GPS/FLP fix for > threshold)
    let gps_flp: Vec<&FixRecord> = sorted
        .iter()
        .filter(|f| f.provider == FixProvider::Gps || f.provider == FixProvider::Flp)
        .copied()
        .collect();

    let nlp_only: Vec<&FixRecord> = sorted
        .iter()
        .filter(|f| f.provider == FixProvider::Nlp)
        .copied()
        .collect();

    println!("  GPS+FLP fixes: {}", gps_flp.len());
    println!("  NLP fixes: {}", nlp_only.len());

    if gps_flp.is_empty() {
        println!("  No GPS/FLP fixes — NLP is the only source.");
        return;
    }

    // Detect gaps in GPS/FLP coverage
    let gap_threshold_s = 5.0; // 5 seconds without GPS/FLP = gap
    let mut gaps: Vec<(i64, i64)> = Vec::new(); // (gap_start_ms, gap_end_ms)

    // Gap before first GPS/FLP fix
    if gps_flp[0].unix_time_ms - t_start > (gap_threshold_s * 1000.0) as i64 {
        gaps.push((t_start, gps_flp[0].unix_time_ms));
    }

    // Gaps between consecutive GPS/FLP fixes
    for i in 1..gps_flp.len() {
        let dt_ms = gps_flp[i].unix_time_ms - gps_flp[i - 1].unix_time_ms;
        if dt_ms as f64 / 1000.0 > gap_threshold_s {
            gaps.push((gps_flp[i - 1].unix_time_ms, gps_flp[i].unix_time_ms));
        }
    }

    // Gap after last GPS/FLP fix
    if t_end - gps_flp[gps_flp.len() - 1].unix_time_ms > (gap_threshold_s * 1000.0) as i64 {
        gaps.push((gps_flp[gps_flp.len() - 1].unix_time_ms, t_end));
    }

    let total_gap_s: f64 = gaps.iter().map(|(s, e)| (e - s) as f64 / 1000.0).sum();
    println!(
        "\n  GPS/FLP gaps (>{gap_threshold_s}s): {} gaps, {total_gap_s:.0}s total ({:.1}% of session)",
        gaps.len(),
        total_gap_s / total_s * 100.0,
    );

    // Classify gaps by duration
    let mut gap_durations: Vec<f64> = gaps.iter().map(|(s, e)| (e - s) as f64 / 1000.0).collect();
    gap_durations.sort_by(|a, b| a.partial_cmp(b).unwrap());

    if !gap_durations.is_empty() {
        let buckets = [
            (5.0, 10.0),
            (10.0, 30.0),
            (30.0, 60.0),
            (60.0, 300.0),
            (300.0, f64::MAX),
        ];
        let labels = ["5-10s", "10-30s", "30-60s", "1-5min", "5min+"];
        print!("  Gap duration distribution: ");
        for (i, &(lo, hi)) in buckets.iter().enumerate() {
            let count = gap_durations.iter().filter(|&&d| d >= lo && d < hi).count();
            if count > 0 {
                print!("{}={} ", labels[i], count);
            }
        }
        println!();

        // Show longest gaps
        let top_n = gap_durations.len().min(5);
        println!("  Longest gaps:");
        for d in gap_durations.iter().rev().take(top_n) {
            println!("    {d:.1}s");
        }
    }

    // NLP fixes during GPS/FLP gaps vs outside
    let mut nlp_in_gap = 0usize;
    let mut nlp_outside_gap = 0usize;
    for nlp in &nlp_only {
        let in_gap = gaps
            .iter()
            .any(|(s, e)| nlp.unix_time_ms >= *s && nlp.unix_time_ms <= *e);
        if in_gap {
            nlp_in_gap += 1;
        } else {
            nlp_outside_gap += 1;
        }
    }

    println!("\n  NLP fix timing:");
    println!("    During GPS/FLP gaps: {nlp_in_gap}");
    println!("    Outside gaps (redundant): {nlp_outside_gap}");

    if nlp_in_gap > 0 {
        // Accuracy of NLP during gaps
        let mut gap_accs: Vec<f64> = nlp_only
            .iter()
            .filter(|f| {
                gaps.iter()
                    .any(|(s, e)| f.unix_time_ms >= *s && f.unix_time_ms <= *e)
            })
            .filter_map(|f| f.accuracy_m)
            .collect();
        gap_accs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if !gap_accs.is_empty() {
            let median = gap_accs[gap_accs.len() / 2];
            let min = gap_accs[0];
            let max = gap_accs[gap_accs.len() - 1];
            println!(
                "    NLP accuracy during gaps: min={min:.0}m, median={median:.0}m, max={max:.0}m"
            );
        }
    }

    // Show a few example gaps with NLP coverage
    println!("\n  Example gaps with NLP coverage (up to 10):");
    let mut shown = 0;
    for (gs, ge) in &gaps {
        let gap_nlps: Vec<&&FixRecord> = nlp_only
            .iter()
            .filter(|f| f.unix_time_ms >= *gs && f.unix_time_ms <= *ge)
            .collect();
        if !gap_nlps.is_empty() && shown < 10 {
            let gap_dur = (*ge - *gs) as f64 / 1000.0;
            let offset_s = (*gs - t_start) as f64 / 1000.0;
            let accs: Vec<String> = gap_nlps
                .iter()
                .take(3)
                .map(|f| format!("{:.0}m", f.accuracy_m.unwrap_or(-1.0)))
                .collect();
            println!(
                "    t+{:.0}s: gap={gap_dur:.1}s, {} NLP fixes (acc: {}{})",
                offset_s,
                gap_nlps.len(),
                accs.join(", "),
                if gap_nlps.len() > 3 { ", ..." } else { "" },
            );
            shown += 1;
        }
    }
}

fn analyze_jumps(fixes: &[FixRecord]) {
    let mut sorted: Vec<&FixRecord> = fixes.iter().collect();
    sorted.sort_by_key(|f| f.unix_time_ms);
    analyze_jumps_ref(&sorted);
}

fn analyze_jumps_ref(fixes: &[&FixRecord]) {
    if fixes.len() < 2 {
        return;
    }

    let mut jump_speeds: Vec<(f64, usize, &FixRecord, &FixRecord)> = Vec::new();
    let mut max_jump_speed = 0.0f64;
    let mut speed_histogram: HashMap<&str, usize> = HashMap::new();

    for i in 1..fixes.len() {
        let prev = fixes[i - 1];
        let curr = fixes[i];

        let dt_s = (curr.unix_time_ms - prev.unix_time_ms) as f64 / 1000.0;
        if dt_s <= 0.0 {
            continue;
        }

        let dist_m = geo::haversine_distance_m(
            prev.latitude_deg,
            prev.longitude_deg,
            curr.latitude_deg,
            curr.longitude_deg,
        );

        let speed_kmh = (dist_m / dt_s) * 3.6;

        let bucket = if speed_kmh < 1.0 {
            "<1 km/h"
        } else if speed_kmh < 10.0 {
            "1-10 km/h"
        } else if speed_kmh < 50.0 {
            "10-50 km/h"
        } else if speed_kmh < 100.0 {
            "50-100 km/h"
        } else if speed_kmh < 200.0 {
            "100-200 km/h"
        } else if speed_kmh < 500.0 {
            "200-500 km/h"
        } else {
            "500+ km/h"
        };
        *speed_histogram.entry(bucket).or_default() += 1;

        if speed_kmh > max_jump_speed {
            max_jump_speed = speed_kmh;
        }

        if speed_kmh > 200.0 {
            jump_speeds.push((speed_kmh, i, prev, curr));
        }
    }

    // Speed histogram
    println!("    Implied speed between consecutive fixes:");
    for bucket in [
        "<1 km/h",
        "1-10 km/h",
        "10-50 km/h",
        "50-100 km/h",
        "100-200 km/h",
        "200-500 km/h",
        "500+ km/h",
    ] {
        let count = speed_histogram.get(bucket).copied().unwrap_or(0);
        if count > 0 {
            println!("      {bucket:15} {count:>8}");
        }
    }
    println!("    Max implied speed: {max_jump_speed:.1} km/h");

    // Show worst jumps
    jump_speeds.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let top = jump_speeds.iter().take(10);
    if jump_speeds.is_empty() {
        println!("    No jumps > 200 km/h detected.");
    } else {
        println!(
            "    Top {} jumps (>{} km/h):",
            jump_speeds.len().min(10),
            200
        );
        for (speed, _idx, prev, curr) in top {
            let dt_s = (curr.unix_time_ms - prev.unix_time_ms) as f64 / 1000.0;
            let dist_m = geo::haversine_distance_m(
                prev.latitude_deg,
                prev.longitude_deg,
                curr.latitude_deg,
                curr.longitude_deg,
            );
            println!(
                "      {speed:>10.1} km/h  dist={dist_m:>10.1}m  dt={dt_s:>6.1}s  {:?}→{:?}  acc={:?}→{:?}",
                prev.provider, curr.provider, prev.accuracy_m, curr.accuracy_m,
            );
        }
    }
}

fn analyze_altitude(fixes: &[FixRecord]) {
    // Only analyze GPS+FLP (Primary) fixes with altitude
    let mut primary: Vec<&FixRecord> = fixes
        .iter()
        .filter(|f| f.provider == FixProvider::Gps || f.provider == FixProvider::Flp)
        .filter(|f| f.altitude_m.is_some())
        .collect();
    primary.sort_by_key(|f| f.unix_time_ms);

    if primary.len() < 2 {
        println!("  Not enough Primary fixes with altitude.");
        return;
    }

    println!("  Primary fixes with altitude: {}", primary.len());

    // --- Vertical accuracy distribution ---
    let mut vert_accs: Vec<f64> = primary
        .iter()
        .filter_map(|f| f.vertical_accuracy_m)
        .collect();
    vert_accs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if !vert_accs.is_empty() {
        let n = vert_accs.len();
        println!(
            "  Vertical accuracy (m): min={:.1}, median={:.1}, p90={:.1}, p95={:.1}, p99={:.1}, max={:.1} (n={})",
            vert_accs[0],
            vert_accs[n / 2],
            vert_accs[(n as f64 * 0.9) as usize],
            vert_accs[(n as f64 * 0.95) as usize],
            vert_accs[(n as f64 * 0.99) as usize],
            vert_accs[n - 1],
            n,
        );
    } else {
        println!("  No vertical_accuracy_m data available.");
    }

    // --- Altitude range ---
    let alts: Vec<f64> = primary.iter().map(|f| f.altitude_m.unwrap()).collect();
    let alt_min = alts.iter().cloned().fold(f64::INFINITY, f64::min);
    let alt_max = alts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    println!(
        "  Altitude range: {alt_min:.1}m .. {alt_max:.1}m (span={:.1}m)",
        alt_max - alt_min
    );

    // --- Vertical velocity between consecutive fixes ---
    struct VJump {
        idx: usize,
        vv: f64, // vertical velocity (m/s), signed
        dt_s: f64,
        d_alt: f64,
        vert_acc_before: Option<f64>,
        vert_acc_after: Option<f64>,
        alt_before: f64,
        alt_after: f64,
        _t_offset_s: f64,
    }

    let t_start = primary[0].unix_time_ms;
    let mut jumps: Vec<VJump> = Vec::new();

    for i in 1..primary.len() {
        let prev = primary[i - 1];
        let curr = primary[i];
        let dt_s = (curr.unix_time_ms - prev.unix_time_ms) as f64 / 1000.0;
        if dt_s <= 0.0 || dt_s > 60.0 {
            continue; // skip zero-time or gap > 60s
        }
        let d_alt = curr.altitude_m.unwrap() - prev.altitude_m.unwrap();
        let vv = d_alt / dt_s;
        jumps.push(VJump {
            idx: i,
            vv,
            dt_s,
            d_alt,
            vert_acc_before: prev.vertical_accuracy_m,
            vert_acc_after: curr.vertical_accuracy_m,
            alt_before: prev.altitude_m.unwrap(),
            alt_after: curr.altitude_m.unwrap(),
            _t_offset_s: (prev.unix_time_ms - t_start) as f64 / 1000.0,
        });
    }

    // Vertical velocity distribution (absolute)
    let mut abs_vv: Vec<f64> = jumps.iter().map(|j| j.vv.abs()).collect();
    abs_vv.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if !abs_vv.is_empty() {
        let n = abs_vv.len();
        println!(
            "\n  Vertical velocity |Δalt/Δt| (m/s): median={:.2}, p90={:.2}, p95={:.2}, p99={:.2}, max={:.2}",
            abs_vv[n / 2],
            abs_vv[(n as f64 * 0.9) as usize],
            abs_vv[(n as f64 * 0.95) as usize],
            abs_vv[(n as f64 * 0.99) as usize],
            abs_vv[n - 1],
        );

        // Spike counts by threshold
        let thresholds = [5.0, 10.0, 20.0, 50.0, 100.0];
        print!("  Spikes by threshold: ");
        for &t in &thresholds {
            let count = abs_vv.iter().filter(|&&v| v > t).count();
            print!(">{t:.0}m/s={count} ");
        }
        println!();
    }

    // --- Top 10 worst altitude spikes ---
    let mut worst: Vec<&VJump> = jumps.iter().collect();
    worst.sort_by(|a, b| b.vv.abs().partial_cmp(&a.vv.abs()).unwrap());

    let n_show = worst.len().min(15);
    println!("\n  Top {n_show} worst vertical jumps:");
    println!(
        "    {:>8} {:>8} {:>8} {:>10} {:>10} {:>8}",
        "vv(m/s)", "Δalt(m)", "dt(s)", "alt_before", "alt_after", "vert_acc"
    );
    for j in worst.iter().take(n_show) {
        println!(
            "    {:>8.1} {:>8.1} {:>8.1} {:>10.1} {:>10.1} {:>8}",
            j.vv,
            j.d_alt,
            j.dt_s,
            j.alt_before,
            j.alt_after,
            match (j.vert_acc_before, j.vert_acc_after) {
                (Some(a), Some(b)) => format!("{a:.1}/{b:.1}"),
                _ => "n/a".to_string(),
            },
        );
    }

    // --- Provider interleaving analysis ---
    let mut gps_alts: Vec<f64> = Vec::new();
    let mut flp_alts: Vec<f64> = Vec::new();
    let mut alt_diffs_gps_flp: Vec<f64> = Vec::new();
    for i in 1..primary.len() {
        let prev = primary[i - 1];
        let curr = primary[i];
        if prev.provider != curr.provider {
            let diff = curr.altitude_m.unwrap() - prev.altitude_m.unwrap();
            alt_diffs_gps_flp.push(diff);
        }
        match curr.provider {
            FixProvider::Gps => gps_alts.push(curr.altitude_m.unwrap()),
            FixProvider::Flp => flp_alts.push(curr.altitude_m.unwrap()),
            _ => {}
        }
    }

    if !alt_diffs_gps_flp.is_empty() {
        alt_diffs_gps_flp.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = alt_diffs_gps_flp.len();
        let abs_diffs: Vec<f64> = alt_diffs_gps_flp.iter().map(|d| d.abs()).collect();
        let mut abs_sorted = abs_diffs.clone();
        abs_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("\n  GPS↔FLP provider switches: {} transitions", n,);
        println!(
            "    |Δalt| at provider switch: median={:.1}m, p90={:.1}m, p95={:.1}m, max={:.1}m",
            abs_sorted[n / 2],
            abs_sorted[(n as f64 * 0.9) as usize],
            abs_sorted[(n as f64 * 0.95) as usize],
            abs_sorted[n - 1],
        );
    }

    // Altitude stats per provider
    if !gps_alts.is_empty() && !flp_alts.is_empty() {
        gps_alts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        flp_alts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "    GPS altitude: min={:.1}, median={:.1}, max={:.1} (n={})",
            gps_alts[0],
            gps_alts[gps_alts.len() / 2],
            gps_alts[gps_alts.len() - 1],
            gps_alts.len()
        );
        println!(
            "    FLP altitude: min={:.1}, median={:.1}, max={:.1} (n={})",
            flp_alts[0],
            flp_alts[flp_alts.len() / 2],
            flp_alts[flp_alts.len() - 1],
            flp_alts.len()
        );
    }

    // --- Spike segment analysis ---
    // A "spike" = sequence where vertical velocity exceeds threshold
    let spike_threshold = 10.0; // m/s
    let mut spike_indices: Vec<usize> = Vec::new();
    for j in &jumps {
        if j.vv.abs() > spike_threshold {
            spike_indices.push(j.idx - 1);
            spike_indices.push(j.idx);
        }
    }
    spike_indices.sort();
    spike_indices.dedup();

    // Group consecutive indices into segments
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut seg_start: Option<usize> = None;
    let mut seg_end: usize = 0;
    for &idx in &spike_indices {
        match seg_start {
            None => {
                seg_start = Some(idx);
                seg_end = idx;
            }
            Some(_) => {
                if idx <= seg_end + 2 {
                    seg_end = idx;
                } else {
                    segments.push((seg_start.unwrap(), seg_end));
                    seg_start = Some(idx);
                    seg_end = idx;
                }
            }
        }
    }
    if let Some(s) = seg_start {
        segments.push((s, seg_end));
    }

    println!(
        "\n  Spike segments (>{spike_threshold:.0} m/s threshold): {} segments, {} total points",
        segments.len(),
        spike_indices.len(),
    );
    if !segments.is_empty() {
        println!(
            "    {:>8} {:>8} {:>10} {:>10} {:>12}",
            "start", "end", "points", "t_offset", "alt_range"
        );
        for (s, e) in segments.iter().take(20) {
            let alts_in_seg: Vec<f64> = (*s..=*e).map(|i| primary[i].altitude_m.unwrap()).collect();
            let seg_min = alts_in_seg.iter().cloned().fold(f64::INFINITY, f64::min);
            let seg_max = alts_in_seg
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max);
            let t_off = (primary[*s].unix_time_ms - t_start) as f64 / 1000.0;
            println!(
                "    {:>8} {:>8} {:>10} {:>9.0}s {:>5.0}-{:.0}m",
                s,
                e,
                e - s + 1,
                t_off,
                seg_min,
                seg_max,
            );
        }
    }
}

fn analyze_dr(path: &str) {
    let file = File::open(path).unwrap();
    let reader = BufReader::new(file);
    let mut processor = GnssProcessor::new();

    for line in reader.lines() {
        let line = line.unwrap();
        processor.process_line(&line);
    }

    let result = processor.finalize();
    let d = &result.dr_diagnostics;

    println!(
        "  GNSS fixes: {} total, {} emitted as trajectory",
        d.gnss_total, d.gnss_emitted
    );
    println!("  Attitude samples: {}", d.attitude_total);
    println!("  IMU samples: {} total", d.imu_total);
    println!("    Integrated:           {:>10}", d.imu_integrated);
    println!("    Rejected (no state):  {:>10}", d.imu_rejected_no_state);
    println!(
        "    Rejected (no att):    {:>10}",
        d.imu_rejected_no_attitude
    );
    println!(
        "    Rejected (stale att): {:>10}",
        d.imu_rejected_stale_attitude
    );
    println!(
        "    Rejected (max dur):   {:>10}",
        d.imu_rejected_max_duration
    );
    println!("    Rejected (min dt):    {:>10}", d.imu_rejected_min_dt);
    println!("    Rejected (gap):       {:>10}", d.imu_rejected_gap);
    println!("  DR trajectory points: {}", result.dr_trajectory.len());

    if d.imu_total > 0 {
        let pct_integrated = d.imu_integrated as f64 / d.imu_total as f64 * 100.0;
        let pct_stale = d.imu_rejected_stale_attitude as f64 / d.imu_total as f64 * 100.0;
        println!(
            "  Integration rate: {:.1}% ({:.1}% stale attitude rejection)",
            pct_integrated, pct_stale
        );
    }
}
