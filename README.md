# battery-monitor

A Rust-based daemon and TUI utility for monitoring battery status and health on Linux.

## Crates

- `battery-monitor-daemon`: background service that polls battery information and stores it in a local [sled](https://github.com/spacejam/sled) database.
- `battery-monitor-viewer`: terminal user interface for inspecting collected statistics.
- `battery-monitor-core`: shared types and configuration loading.

## Configuration

Configuration is loaded from `config.toml` in the working directory. Example:

```toml
poll_interval_secs = 60
# battery = "BAT0"
database_path = "battery.db"
```

## Building

```bash
cargo build --workspace
```

## Running

```bash
cargo run --bin battery-monitor-daemon
# In another terminal
cargo run --bin battery-monitor-viewer
```

## Nix

A simple `flake.nix` is provided. You can run the tools via:

```bash
nix run .#daemon
nix run .#viewer
```

A development shell with Rust tooling is available with:

```bash
nix develop
```
