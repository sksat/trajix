# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is trajix?

A GNSS Logger (Android) data visualization web app. Parses 1.2GB+ log files in-browser via WASM, stores results in DuckDB-wasm, and visualizes positioning quality on 3D maps (CesiumJS with GSI terrain), sky plots, time series charts, and constellation analysis views. Includes IMU-based Dead Reckoning for GNSS-degraded segments.

## Build & Test Commands

```bash
# Build
cargo build

# Run all tests
cargo test

# Run a specific test
cargo test parse_nlp_empty_fields

# Run tests for a specific crate
cargo test -p trajix-core

# CLI: analyze a GNSS log file
cargo run -p trajix-cli --release -- gnss_log_*.txt
```

## CLI for Data Analysis

`trajix-cli` provides the same parsing as the browser pipeline but runs locally on the command line. Use it to:
- Investigate data quality issues (accuracy distribution, spatial outliers) without browser E2E
- Validate filtering/processing logic changes with fast feedback
- Prototype new analysis before adding to the web UI

When developing new data processing features (filtering, aggregation, etc.), prefer testing via CLI first to avoid unnecessarily E2E-heavy workflows. Add analysis subcommands to the CLI as needed.

## Architecture

**Rust workspace** crates:
- `crates/trajix-core/` — Parser library. Parses GNSS Logger CSV format into typed records. Compiles to WASM.
- `crates/trajix-wasm/` — WASM bindings. Exposes streaming `feed(chunk)` API for browser use.
- `crates/trajix-cli/` — CLI tool for data analysis without browser. Useful for investigating data quality issues on real log files.
- `web/` — React + TypeScript frontend (Vite + CesiumJS).

**Data pipeline**: File D&D → Web Worker → WASM parser → Arrow RecordBatch → DuckDB-wasm → React UI

### trajix-core internal structure

- `types.rs` — Shared enums: `ConstellationType` (GPS/GLONASS/QZSS/BeiDou/Galileo), `FixProvider` (GPS/FLP/NLP), `CodeType`, `RecordType`
- `error.rs` — `ParseError` with `thiserror`
- `record/` — One file per record type. Each struct has a `parse(line: &str) -> Result<Self, ParseError>` method that splits CSV and handles empty fields as `Option`.
- `parser/` — `header.rs` parses `# ` comment lines for device metadata. Future: `streaming.rs` for `BufRead`-based line-by-line parsing with time context tracking for Status records (their `UnixTimeMillis` is always empty).

### GNSS Logger data format

CSV lines prefixed by record type: `Fix,`, `Status,`, `Raw,`, `UncalAccel,`, etc. Key gotchas:
- **Status records have empty UnixTimeMillis** — must infer from neighboring records
- **NLP Fix records** often have empty altitude, speed, bearing fields
- **QZSS Raw records** lack ECEF satellite positions (36K records)
- **NLP accuracy=400.0** is a fallback/sentinel value
- Field counts are stable: Fix=17, Raw=54, Status=14

## Test Fixtures

Real data extracted from `gnss_log_*.txt` lives in `crates/trajix-core/tests/fixtures/`. Loaded in tests via:
```rust
let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
std::fs::read_to_string(path).unwrap()
```

Fixtures cover normal cases, edge cases (empty fields, fallback values, missing ECEF), and different file positions (start/mid/end).

## Development Approach

- **TDD**: Write tests first against fixture data, then implement parsing
- **Incremental commits**: Small, focused commits for each record type or feature
- **Use latest dependency versions**
- See `DESIGN.md` for full architecture, DuckDB schema, visualization design, and implementation roadmap
