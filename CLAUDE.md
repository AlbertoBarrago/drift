# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project

`drift` is a local, deterministic history and causal-diagnostics engine for Linux workstations (first target: Arch Linux). It stores bounded normalized snapshots, computes semantic diffs against a `known-good` baseline, derives a dependency graph, and applies explainable rules to explain what changed.

Hard product constraints, they are not negotiable without an explicit decision:

- No LLM decides system health. Rules are deterministic Rust code.
- No machine mutation. There is no repair, downgrade, or rollback implementation, and none may be added without a proposed-action, dry-run, confirmation, validation, and undo design.
- No cloud, accounts, telemetry, or remote control.
- No causation claimed from temporal correlation alone. Package and kernel changes are evidence, not diagnoses.
- Never parse human-oriented command output when a machine-oriented interface exists.

Note: the development host here is macOS. The full workspace compiles and tests pass on macOS because all tests use fake probes and fixtures, but real collectors only produce meaningful data on Arch Linux. Validate collector changes in a disposable Arch VM (see `docs/TESTING.md`).

## Commands

```bash
cargo build
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings      # CI gate, warnings are errors
cargo run -p drift -- --help
cargo test -p drift-engine --test fixtures   # fixture-driven diagnosis tests
```

CI (`.github/workflows/ci.yml`) runs fmt, test, clippy on every PR. Run all three locally before declaring work done.

## Workspace layout

Rust 2024 edition, MSRV 1.85, cargo workspace. Dependencies point strictly inward:

```text
drift-domain           normalized facts, StateChange, Diagnosis, DependencyGraph. No I/O.
drift-engine           pure diff, graph derivation, core rules, DiagnosisProfile trait. Depends only on domain.
drift-collect          the only crate that runs commands or touches the filesystem. SystemProbe/CommandRunner traits.
drift-store            SQLite repository, bincode + zstd payloads.
drift-profile-arch     Arch extension point (currently an empty DiagnosisProfile).
drift-profile-omarchy  Omarchy/Waybar collectors, graph edges, rules.
drift-cli              composition root and all presentation. Binary name: `drift`.
```

The engine must never execute a command, open SQLite, or print. The domain must never depend on anything else in the workspace.

## Key invariants

- `SystemSnapshot::new` computes a blake3 `content_hash` over an explicitly enumerated tuple of facts, and the `id` is its first 16 hex chars. Adding a field to `SnapshotFacts` requires adding it to `compute_hash`, otherwise two different machines hash identically. Facts live in ordered collections (`BTreeMap`/`BTreeSet`) so hashing is deterministic.
- `SNAPSHOT_SCHEMA_VERSION` in `drift-domain` must be bumped on any breaking change to the persisted fact shape, since payloads are bincode-encoded blobs in SQLite.
- Every collector returns a `CollectorReport` with outcome `Complete`, `Partial`, `Unavailable`, or `Failed`. A missing tool or missing privilege must surface as `Unavailable`, never as an empty successful fact set. Silent fallbacks are bugs.
- Diagnosis confidence is `High`, `Medium`, or `Low`. Never invent numeric precision. `High` requires both a directly observed failure path and a relevant state change.
- Diagnosis IDs embed the baseline and current snapshot IDs (`rule@<good-id>@<current-id>[@detail]`), which is how `drift explain` reconstructs evidence. Keep that format when adding rules.
- Rule IDs are versioned (`docker-service-disabled/v1`). Change the behavior, bump the version.
- Privacy allowlist in `docs/PRIVACY.md` is binding: no hostnames, usernames, machine IDs, MAC/IP addresses, process arguments, raw command output, or file contents. Config facts store hash, existence, and size only.

## Adding a diagnosis rule

1. Add the normalized facts to `drift-domain` (and to `compute_hash`) if new observations are needed.
2. Collect them in `drift-collect` or in a profile's `SnapshotExtension`, with an honest `CollectorReport`.
3. Emit the semantic change from `drift_engine::diff`.
4. Implement the rule in `drift-engine` (distribution-neutral) or in a profile crate (distribution-specific).
5. Add a known-good fixture and a broken fixture under `fixtures/`, wire them into `crates/drift-engine/tests/fixtures.rs`, and assert both the exact semantic change and the diagnosis confidence plus evidence.
6. Run the full gate: fmt, test, clippy.

Tests must never read the developer workstation. Use the fixture JSON shape already defined in `crates/drift-engine/tests/fixtures.rs`.

## Runtime state

Snapshots go to `$XDG_STATE_HOME/drift` or `~/.local/state/drift`, in `snapshots.sqlite3`. The `tags` table holds the `known-good` marker. Treat that directory as sensitive local data, never attach it to an issue.

## Conventions

- Conventional Commits, English, atomic.
- Comments and identifiers in English, explaining non-obvious decisions only.
- No new dependency without a stated operational need.
- Keep PRs small, one scope each.
