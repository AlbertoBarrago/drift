# Testing on Linux

## Fast deterministic suite

Run on any supported development host:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

These tests use fake probes and versioned facts. They must never query the developer workstation.

## Arch Linux virtual machine

Use a disposable Arch Linux VM to validate real collectors. A VM is preferred over a primary workstation because collector compatibility work may reveal assumptions about installed commands, systemd scope, permissions, and desktop configuration.

Suggested matrix:

| Scenario | Expected result |
| --- | --- |
| Minimal Arch, no Docker | unavailable tools are explicit, no false diagnosis |
| Arch with Docker enabled | snapshot and known-good complete |
| Disable `docker.service` after known-good | HIGH Docker diagnosis |
| Hyprland/Waybar with an invalid interface | Omarchy Waybar diagnosis |
| Kernel and driver change | semantic diff only until acceleration failure is observed |

Do not test repair or rollback behavior on a host with irreplaceable data.

## Containers

An Arch container is useful for compiling and unit tests, but it is not a substitute for a systemd or Hyprland VM. Containers cannot validate user systemd, desktop configuration, live ports, or GPU drivers.

## Adding a regression fixture

1. Add a normalized JSON file under `fixtures/`.
2. Load it from an integration test.
3. Assert the exact semantic change.
4. Assert diagnosis confidence and evidence when appropriate.
5. Run the full local quality gate.
