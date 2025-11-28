# Repository Guidelines

## Project Structure & Module Organization
- `battery_monitor/`: core library. `cli.py` (Typer commands), `collector.py` (reads `/sys/class/power_supply`), `db.py` (SQLite schema/helpers), `graph.py` (matplotlib plots), `sysfs.py` (kernel reads).
- `tests/`: pytest suite (`test_cli.py`, `test_collector.py`, `test_db.py`, `test_sysfs.py`) with fixtures in `conftest.py`.
- `systemd/`: sample service/timer units for periodic collection.
- `battery_*.png`: generated report artifacts; safe to delete and excluded from tests.

## Build, Test, and Development Commands
- `nix develop`: enter the dev shell (fish) with Python, pytest, ruff, typos, pre-commit.
- `nix run . -- collect --help`: show CLI help; swap `--help` for `collect` or `report` to run.
- `python -m pytest` (inside dev shell) or `nix develop -c pytest`: run tests.
- `nix develop -c pre-commit run --all-files`: lint (ruff) + spell-check (typos) before commits.
- `battery-monitor-report --timeframe last_day --graph`: smoke test reporting when a DB exists.

## Coding Style & Naming Conventions
- Python 3.10+, 4-space indentation, type hints preferred; keep modules single-purpose.
- Patterns: CLI in `cli.py`, IO in `collector.py`/`sysfs.py`, DB access in `db.py`, rendering in `graph.py`.
- Naming: snake_case for code, lowercase-dash for CLI flags, safe filenames via `_default_graph_path`.
- Run `ruff check .` and `typos` (pre-commit) to keep formatting and spelling consistent.

## Testing Guidelines
- Tests live in `tests/test_*.py`; mirror new modules with matching tests.
- Use fixtures in `tests/conftest.py` for temp DBs; mock `sysfs` helpers instead of real `/sys`.
- Add regression tests for new timeframes, schema changes, or CLI flags; cover edge cases (no samples, multiple batteries).
- Capture expected CLI snippets with `runner.invoke` where it guards behavior.

## Commit & Pull Request Guidelines
- Use short, imperative subjects (e.g., “Simplify image output CLI”); add body for rationale when helpful.
- Keep commits cohesive (code + tests + docs) and ensure pre-commit passes.
- PRs: describe scope, manual test notes (commands run, outputs/paths), linked issues, and graph screenshots when behavior changes.

## Configuration & Deployment Notes
- Default DB: `~/.local/share/battery-monitor/battery.db`; override with `--db` or `BATTERY_MONITOR_DB`.
- Sample systemd units run collection every 5 minutes; adjust paths/env vars before installing to `/etc/systemd/system/` or `~/.config/systemd/user/`.
- Graphs default to the current directory; use `--graph-path` when scripting to avoid clutter.
