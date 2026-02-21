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
