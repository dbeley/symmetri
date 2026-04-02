# Maintenance Plan for symmetri

## Overview
Comprehensive maintenance pass covering bugs, performance, refactoring, and cleanup across the entire codebase.

## Execution Order (14 commits, one per issue)

### Phase 1: Foundational Changes
1. **Fix MetricKind deduplication** - Use `strum` derive macros to eliminate triple-duplicated `as_str()`, `from_label()`, and `FromStr` implementations in `metrics.rs`. Add `strum` dependency.

2. **Fix `fetch_metric_samples` SQL filter** - Push `kinds` filter into WHERE clause instead of loading ALL rows then filtering in Rust. Build dynamic SQL with placeholders.

3. **Fix `fetch_latest_metric_samples`** - Replace full-table scan + Rust dedup with SQL `GROUP BY kind, source` with `MAX(ts)` subquery.

4. **Fix DB connection reuse** - Add `count_metric_samples_with_conn`, `fetch_metric_samples_with_conn`, `fetch_latest_metric_samples_with_conn` variants. Update `cli.rs` report path to open one connection and reuse it.

### Phase 2: Bug Fixes
5. **Fix `collect_loop` exit code discard** - Change `let _ = collect_once(...)?;` to properly handle the `Result<i32>` return value. Log non-zero exit codes.

6. **Replace `std::process::exit(1)` in cli.rs** - Return errors via `anyhow::Result` instead of calling `exit()` directly. The `main.rs` already handles `Err` with `exit(1)`.

### Phase 3: Refactoring
7. **Fix duplicated preset logic** - Create a single source of truth for preset→kinds mapping. Derive `has_data_for_preset` and `metric_kinds_for_presets` from it.

8. **Deduplicate table functions** - Create generic `usage_stats_table()` to replace identical `memory_stats_table`/`disk_stats_table`. Create generic `freq_usage_table()` for `cpu_stats_table`/`gpu_stats_table`.

9. **Fix fragile status detection** - Replace `.contains("dis")` hack with explicit enum matching or direct equality checks.

10. **Split cli.rs** - Extract report/table rendering into `src/report.rs`. Keep CLI parsing and dispatch in `cli.rs`.

### Phase 4: Cleanup
11. **Fix lib.rs visibility** - Change `pub mod` to private `mod` for internal modules. Only re-export public API via `pub use`.

12. **Fix `format_bucket` multi-day label** - Compute days from the actual date span, not from bucket_seconds.

13. **Fix `metric_from_row` error type** - Create a proper error type instead of misusing `std::fmt::Error`.

14. **Minor cleanup** - Remove empty `repository = ""`, fix `.gitignore` duplicate `target`, update `flake.nix` version to match `Cargo.toml`, add `[profile.release]`.

## Verification
After each commit: `nix develop -c cargo test` must pass.
After all commits: `nix develop -c cargo clippy --all-targets --all-features` and `nix develop -c prek run --all-files` must pass.
