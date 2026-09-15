# Architecture

## Design constraints

- Deterministic and explainable conclusions.
- Local-first operation and bounded collection.
- No hidden machine mutation.
- Stable, testable domain boundaries.

## Workspace

```text
drift-domain           normalized facts and public domain types
drift-engine           pure diff, graph derivation, core rules
drift-collect          command and filesystem boundary, collectors
drift-store            SQLite snapshot repository
drift-profile-arch     Arch-specific extension point
drift-profile-omarchy  Omarchy collectors, graph edges, and rules
drift-cli              composition root and presentation
```

Dependencies point inward. The engine does not execute commands, access SQLite, or render CLI output.

## Snapshot lifecycle

```text
SystemProbe -> Collectors -> SystemSnapshot -> SQLite
                                      |             |
                                      v             v
                                  StateDiff <- known-good tag
                                      |
                                      v
                         DependencyGraph + diagnosis profiles
                                      |
                                      v
                             Diagnosis with explicit evidence
```

`SystemSnapshot` is a typed, immutable observation. Every collector reports `Complete`, `Partial`, `Unavailable`, or `Failed`. Missing privileges or tools never become an empty successful fact set.

## Storage

SQLite indexes snapshot metadata and tags. Snapshot payloads use bincode and zstd. Snapshot IDs and hashes are deterministic for the observed content. The store does not retain raw command output.

## Profiles

Profiles are compiled Rust crates, not dynamic plugins. `drift-profile-arch` and `drift-profile-omarchy` can add collection extensions, graph edges, and diagnosis rules while the generic engine stays distribution-neutral.

## Diagnosis rules

A rule may emit `HIGH`, `MEDIUM`, or `LOW`, never fabricated numeric precision. High confidence requires a direct observed failure path and a relevant state change. Package and kernel changes alone are evidence, not diagnoses.

## Mutations

No mutation implementation exists. Future mutations must model proposed action, confirmation, preconditions, validation, and undo before changing the machine.
