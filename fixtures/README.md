# Fixture machines

These JSON files are intentionally bounded fake-machine inputs. They contain only normalized facts used by deterministic tests, never raw command output or user configuration content.

- `healthy_arch.json`: known-good baseline.
- `docker_disabled.json`: Docker service disablement after the baseline.
- `broken_waybar_interface.json`: Waybar refers to a missing interface.
- `node_version_regression.json`: Node runtime regresses from the known-good major version.
- `kernel_driver_regression.json`: kernel and GPU driver change together.
