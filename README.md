# Drift

`drift` is a local, deterministic history and causal-diagnostics engine for Linux workstations.

It answers a narrow but important question:

> What changed since this machine last worked, and which observed change most likely explains the current failure?

It is not a collection of shell health checks. It stores bounded, normalized workstation snapshots, computes semantic diffs, derives explicit dependency relationships, and applies explainable rules. It never uses an LLM to decide system health and never modifies the machine.

The first supported target is Arch Linux. Omarchy and Hyprland support is an optional compiled profile, not a core assumption.

## Status

This repository is pre-beta. The Docker and Waybar diagnosis paths are working vertical slices, with deterministic fixture coverage. The project is not ready to recommend repairs or package rollback.

See [architecture](docs/ARCHITECTURE.md), [testing](docs/TESTING.md), [privacy](docs/PRIVACY.md), and [contributing](CONTRIBUTING.md).

## What works today

- Local compressed SQLite snapshots and `known-good` markers.
- Semantic diff for kernel, pacman packages, services, processes, GPU driver modules, network interfaces, listening ports, configuration fingerprints, and runtime versions.
- Bounded pacman transaction history.
- System and user systemd service collection.
- Core Docker dependency graph and high-confidence diagnosis when `docker.service` is disabled after a known-good snapshot.
- Omarchy profile support for Waybar references to missing network interfaces.
- Versioned fake-machine fixtures for Docker, Waybar, Node, and kernel/driver regressions.

## Non-goals

- Cloud synchronization, accounts, telemetry, dashboards, or remote control.
- LLM-based health decisions.
- Unattended repair, package downgrade, or arbitrary rollback.
- Broad distribution support in the first releases.

## Quick start

Requirements:

- Rust 1.85 or newer.
- An Arch Linux workstation for meaningful collection.

```bash
cargo build
cargo run -p drift -- --help
cargo test --workspace
```

On an Omarchy workstation, select the profile explicitly:

```bash
cargo run -p drift -- --profile omarchy snapshot
cargo run -p drift -- --profile omarchy mark-good
cargo run -p drift -- --profile omarchy diagnose
```

Snapshots are stored in `$XDG_STATE_HOME/drift` or `~/.local/state/drift`.

## CLI

```text
drift [--profile arch|omarchy] snapshot
drift [--profile arch|omarchy] snapshots
drift [--profile arch|omarchy] mark-good
drift diff <from> [to] [--json]
drift status
drift diagnose [--no-store]
drift explain <diagnosis-id>
drift timeline
drift inspect [target]
```

`diagnose` stores its observed current snapshot by default. `explain` reconstructs evidence from the baseline and current snapshot identifiers embedded in the diagnosis ID.

## Development

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a change. Test strategies for Arch Linux VMs and containers are in [docs/TESTING.md](docs/TESTING.md).

## License

[MIT](LICENSE-MIT)
