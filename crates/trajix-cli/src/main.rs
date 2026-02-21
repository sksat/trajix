use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;

use trajix_core::parser::line::Record;
use trajix_core::parser::streaming::StreamingParser;
use trajix_core::record::fix::FixRecord;
use trajix_core::types::FixProvider;

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
}

fn analyze_fixes(fixes: &[FixRecord]) {
    if fixes.is_empty() {
        println!("No Fix records found.");
        return;
    }

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

            println!("    Accuracy (m): min={min:.1}, median={median:.1}, p90={p90:.1}, p95={p95:.1}, p99={p99:.1}, max={max:.1}");

            // Histogram buckets
            let buckets = [5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0];
            print!("    Histogram: ");
            let mut prev = 0.0;
            for &b in &buckets {
                let count = accuracies
                    .iter()
                    .filter(|&&a| a >= prev && a < b)
                    .count();
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
    println!("  Total time span: {:.0}s ({:.1} hours)", total_s, total_s / 3600.0);

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
        let buckets = [(5.0, 10.0), (10.0, 30.0), (30.0, 60.0), (60.0, 300.0), (300.0, f64::MAX)];
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
            println!("    NLP accuracy during gaps: min={min:.0}m, median={median:.0}m, max={max:.0}m");
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

        let dist_m = haversine_m(
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
            let dist_m = haversine_m(
                prev.latitude_deg,
                prev.longitude_deg,
                curr.latitude_deg,
                curr.longitude_deg,
            );
            println!(
                "      {speed:>10.1} km/h  dist={dist_m:>10.1}m  dt={dt_s:>6.1}s  {:?}→{:?}  acc={:?}→{:?}",
                prev.provider, curr.provider,
                prev.accuracy_m, curr.accuracy_m,
            );
        }
    }
}

/// Haversine distance in meters
fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let lat1r = lat1.to_radians();
    let lat2r = lat2.to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1r.cos() * lat2r.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    R * c
}
