use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

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
    print_fixes(&fixes);

    // Altitude spike analysis
    println!("\n=== Altitude Analysis ===");
    print_altitude(&fixes);

    // Dead Reckoning pipeline analysis (second pass)
    println!("\n=== Dead Reckoning Pipeline ===");
    print_dr(&path);
}

fn print_fixes(fixes: &[FixRecord]) {
    if fixes.is_empty() {
        println!("No Fix records found.");
        return;
    }

    // Summary via library stats API
    let summary = trajix::summarize_fixes(fixes);
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
            "  Accuracy (m): min={:.1}, median={:.1}, p90={:.1}, p95={:.1}, p99={:.1}, max={:.1}",
            acc.min, acc.median, acc.p90, acc.p95, acc.p99, acc.max
        );
    }
    for pc in &summary.per_provider {
        println!("  {}: {} fixes", pc.provider.as_str(), pc.count);
    }
    println!();

    // Fix quality classification
    let qualities = trajix::classify_fixes(fixes, trajix::DEFAULT_GAP_THRESHOLD_MS);
    let mut primary = 0usize;
    let mut gap_fallback = 0usize;
    let mut rejected = 0usize;
    for q in &qualities {
        match q {
            FixQuality::Primary => primary += 1,
            FixQuality::GapFallback => gap_fallback += 1,
            FixQuality::Rejected => rejected += 1,
        }
    }
    println!("=== Fix Quality Classification ===");
    println!("  Primary (GPS+FLP): {primary}");
    println!("  GapFallback (NLP during gap): {gap_fallback}");
    println!("  Rejected (NLP redundant): {rejected}");
    println!();

    // Per-provider detailed stats
    let provider_stats = trajix::provider_detailed_stats(fixes);
    println!("=== Fix Records ({} total) ===", fixes.len());
    for ps in &provider_stats {
        println!(
            "\n  --- {} ({} records) ---",
            ps.provider.as_str(),
            ps.count
        );
        if let Some(ref acc) = ps.accuracy {
            println!(
                "    Accuracy (m): min={:.1}, median={:.1}, p90={:.1}, p95={:.1}, p99={:.1}, max={:.1}",
                acc.min, acc.median, acc.p90, acc.p95, acc.p99, acc.max
            );
        }
        if ps.missing_altitude > 0 || ps.missing_speed > 0 || ps.missing_bearing > 0 {
            println!(
                "    Missing: alt={}, speed={}, bearing={}",
                ps.missing_altitude, ps.missing_speed, ps.missing_bearing
            );
        }
    }

    // Coverage gap analysis
    println!("\n=== Coverage Gap Analysis ===");
    print_coverage(fixes);

    // Consecutive fix jump analysis (all providers mixed)
    println!("\n=== Consecutive Fix Jumps ===");
    print_jumps(fixes);

    // Per-provider jump analysis
    for provider in [FixProvider::Gps, FixProvider::Flp, FixProvider::Nlp] {
        let mut provider_fixes: Vec<FixRecord> = fixes
            .iter()
            .filter(|f| f.provider == provider)
            .cloned()
            .collect();
        if provider_fixes.is_empty() {
            continue;
        }
        provider_fixes.sort_by_key(|f| f.unix_time_ms);
        println!("\n  --- Jumps within {} only ---", provider.as_str());
        let report = trajix::analyze_jumps(&provider_fixes, &trajix::JumpConfig::default());
        print_jump_report(&report);
    }
}

fn print_coverage(fixes: &[FixRecord]) {
    let report = trajix::analyze_coverage(fixes, &trajix::GapConfig::default());

    println!(
        "  Total time span: {:.0}s ({:.1} hours)",
        report.total_time_s,
        report.total_time_s / 3600.0
    );
    println!("  GPS+FLP fixes: {}", report.gps_flp_count);
    println!("  NLP fixes: {}", report.nlp_count);

    if report.gps_flp_count == 0 {
        println!("  No GPS/FLP fixes — NLP is the only source.");
        return;
    }

    println!(
        "\n  GPS/FLP gaps (>5s): {} gaps, {:.0}s total ({:.1}% of session)",
        report.gaps.len(),
        report.total_gap_s,
        report.gap_percentage,
    );

    if !report.gaps.is_empty() {
        // Duration distribution
        let buckets = [
            (5.0, 10.0, "5-10s"),
            (10.0, 30.0, "10-30s"),
            (30.0, 60.0, "30-60s"),
            (60.0, 300.0, "1-5min"),
            (300.0, f64::MAX, "5min+"),
        ];
        print!("  Gap duration distribution: ");
        for &(lo, hi, label) in &buckets {
            let count = report
                .gaps
                .iter()
                .filter(|g| g.duration_s >= lo && g.duration_s < hi)
                .count();
            if count > 0 {
                print!("{label}={count} ");
            }
        }
        println!();

        // Longest gaps
        let mut durations: Vec<f64> = report.gaps.iter().map(|g| g.duration_s).collect();
        durations.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let top_n = durations.len().min(5);
        println!("  Longest gaps:");
        for d in durations.iter().rev().take(top_n) {
            println!("    {d:.1}s");
        }
    }

    // NLP timing
    let nlp = &report.nlp_analysis;
    println!("\n  NLP fix timing:");
    println!("    During GPS/FLP gaps: {}", nlp.nlp_in_gap);
    println!("    Outside gaps (redundant): {}", nlp.nlp_outside_gap);
    if let Some(ref acc) = nlp.gap_nlp_accuracy_stats {
        println!(
            "    NLP accuracy during gaps: min={:.0}m, median={:.0}m, max={:.0}m",
            acc.min, acc.median, acc.max
        );
    }
}

fn print_jumps(fixes: &[FixRecord]) {
    let mut sorted = fixes.to_vec();
    sorted.sort_by_key(|f| f.unix_time_ms);
    let report = trajix::analyze_jumps(&sorted, &trajix::JumpConfig::default());
    print_jump_report(&report);
}

fn print_jump_report(report: &trajix::JumpReport) {
    if report.total_pairs == 0 {
        return;
    }

    println!("    Implied speed between consecutive fixes:");
    for bucket in &report.histogram {
        if bucket.count > 0 {
            println!("      {:15} {:>8}", bucket.label, bucket.count);
        }
    }
    println!("    Max implied speed: {:.1} km/h", report.max_speed_kmh);

    if report.anomalies.is_empty() {
        println!("    No jumps > 200 km/h detected.");
    } else {
        let n = report.anomalies.len().min(10);
        println!("    Top {n} jumps (>200 km/h):");
        for a in report.anomalies.iter().take(10) {
            println!(
                "      {:>10.1} km/h  dist={:>10.1}m  dt={:>6.1}s  {:?}→{:?}  acc={:?}→{:?}",
                a.speed_kmh,
                a.distance_m,
                a.dt_s,
                a.provider_before,
                a.provider_after,
                a.accuracy_before,
                a.accuracy_after,
            );
        }
    }
}

fn print_altitude(fixes: &[FixRecord]) {
    // Filter to GPS+FLP with altitude, sorted by time
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

    // Vertical accuracy distribution
    let mut vert_accs: Vec<f64> = primary
        .iter()
        .filter_map(|f| f.vertical_accuracy_m)
        .collect();
    vert_accs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if !vert_accs.is_empty() {
        let p = trajix::percentiles(&vert_accs);
        println!(
            "  Vertical accuracy (m): min={:.1}, median={:.1}, p90={:.1}, p95={:.1}, p99={:.1}, max={:.1} (n={})",
            p.min,
            p.median,
            p.p90,
            p.p95,
            p.p99,
            p.max,
            vert_accs.len(),
        );
    } else {
        println!("  No vertical_accuracy_m data available.");
    }

    // Altitude range
    let alts: Vec<f64> = primary.iter().map(|f| f.altitude_m.unwrap()).collect();
    let alt_min = alts.iter().cloned().fold(f64::INFINITY, f64::min);
    let alt_max = alts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    println!(
        "  Altitude range: {alt_min:.1}m .. {alt_max:.1}m (span={:.1}m)",
        alt_max - alt_min
    );

    // Vertical velocity analysis via core library
    let timestamps: Vec<i64> = primary.iter().map(|f| f.unix_time_ms).collect();
    let altitudes: Vec<f64> = primary.iter().map(|f| f.altitude_m.unwrap()).collect();
    let vv_report = trajix::analyze_vertical_velocity(
        &timestamps,
        &altitudes,
        &trajix::VerticalVelocityConfig::default(),
    );

    if let Some(ref stats) = vv_report.abs_velocity_stats {
        println!(
            "\n  Vertical velocity |Δalt/Δt| (m/s): median={:.2}, p90={:.2}, p95={:.2}, p99={:.2}, max={:.2}",
            stats.median, stats.p90, stats.p95, stats.p99, stats.max
        );
    }

    // Spike counts
    if !vv_report.spike_counts.is_empty() {
        print!("  Spikes by threshold: ");
        for &(threshold, count) in &vv_report.spike_counts {
            print!(">{threshold:.0}m/s={count} ");
        }
        println!();
    }

    // Top worst vertical jumps
    let mut worst: Vec<&trajix::VerticalVelocity> = vv_report.velocities.iter().collect();
    worst.sort_by(|a, b| {
        b.velocity_mps
            .abs()
            .partial_cmp(&a.velocity_mps.abs())
            .unwrap()
    });
    let n_show = worst.len().min(15);
    println!("\n  Top {n_show} worst vertical jumps:");
    println!(
        "    {:>8} {:>8} {:>8} {:>10} {:>10}",
        "vv(m/s)", "Δalt(m)", "dt(s)", "alt_before", "alt_after"
    );
    for v in worst.iter().take(n_show) {
        println!(
            "    {:>8.1} {:>8.1} {:>8.1} {:>10.1} {:>10.1}",
            v.velocity_mps, v.delta_alt_m, v.dt_s, v.alt_before_m, v.alt_after_m,
        );
    }

    // Provider interleaving analysis via core library
    let primary_owned: Vec<FixRecord> = primary.into_iter().cloned().collect();
    let interleaving = trajix::analyze_provider_interleaving(&primary_owned);

    if let Some(ref stats) = interleaving.abs_delta_alt_stats {
        println!(
            "\n  GPS↔FLP provider switches: {} transitions",
            interleaving.transition_count
        );
        println!(
            "    |Δalt| at provider switch: median={:.1}m, p90={:.1}m, p95={:.1}m, max={:.1}m",
            stats.median, stats.p90, stats.p95, stats.max
        );
    }

    if let (Some(gps), Some(flp)) = (
        &interleaving.gps_altitude_stats,
        &interleaving.flp_altitude_stats,
    ) {
        println!(
            "    GPS altitude: min={:.1}, median={:.1}, max={:.1}",
            gps.min, gps.median, gps.max
        );
        println!(
            "    FLP altitude: min={:.1}, median={:.1}, max={:.1}",
            flp.min, flp.median, flp.max
        );
    }

    // Spike segments
    println!(
        "\n  Spike segments (>10 m/s threshold): {} segments",
        vv_report.spike_segments.len(),
    );
    if !vv_report.spike_segments.is_empty() {
        let t_start = timestamps[0];
        println!(
            "    {:>8} {:>8} {:>10} {:>10} {:>12}",
            "start", "end", "points", "t_offset", "alt_range"
        );
        for &(s, e) in vv_report.spike_segments.iter().take(20) {
            let seg_min = altitudes[s..=e]
                .iter()
                .cloned()
                .fold(f64::INFINITY, f64::min);
            let seg_max = altitudes[s..=e]
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max);
            let t_off = (timestamps[s] - t_start) as f64 / 1000.0;
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

fn print_dr(path: &str) {
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
