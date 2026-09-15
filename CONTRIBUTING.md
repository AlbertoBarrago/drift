# Contributing

Thanks for helping make workstation diagnostics more reliable.

## Before opening an issue

- Search existing issues and discussions once the public repository is available.
- Do not include snapshots, command output, hostnames, IP addresses, tokens, or configuration content in public reports.
- Prefer a minimal fake-machine fixture over output from a real workstation.

## Development setup

Install Rust 1.85 or newer, then run:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Use an Arch Linux VM for collector validation. See [docs/TESTING.md](docs/TESTING.md).

## Change expectations

- Keep pull requests small and focused.
- Add or update a fixture for every new diagnosis rule or regression.
- Keep domain, diff, graph, diagnosis, collector, storage, and CLI concerns separate.
- Never parse human-oriented command output when a machine-oriented interface exists.
- Do not add a dependency without explaining its operational need.
- Do not add a rule that claims causation from temporal correlation alone.
- Never add machine mutation without an explicit proposed-action, dry-run, confirmation, validation, and undo design.

## Tests

Tests must not read the developer workstation. A diagnosis needs at least:

1. A known-good fixture.
2. A changed or broken fixture.
3. A semantic diff assertion.
4. A diagnosis confidence and evidence assertion, when a diagnosis is expected.

Fixture data belongs in `fixtures/` and must contain normalized facts only.

## Commit and pull request guidance

Use Conventional Commit prefixes when commits are introduced:

```text
feat: add systemd user-unit collector
fix: preserve unavailable collector state
test: cover Waybar interface regression
docs: document Arch VM test flow
```

The pull request description should state the problem and scope, observable facts used, privacy impact, tests run, and whether it changes collection, diagnosis, or mutation behavior.

## Code of conduct

Be respectful, specific, and review ideas on technical merit. Security and privacy concerns are always in scope.
