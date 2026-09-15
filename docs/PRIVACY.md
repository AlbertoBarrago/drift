# Privacy and data handling

Snapshots are local by default. They can still reveal workstation characteristics, so treat the state directory as sensitive local data.

## Stored facts

- Installed package names, versions, and install reason.
- Bounded normalized pacman transactions.
- Unit names and states.
- Aggregated executable names and counts.
- Kernel release, runtime versions, GPU driver module names.
- Interface names and operational state.
- Port number, protocol, and bind scope.
- Hashes, existence, and size for an explicit configuration allowlist.

## Never stored by default

- Usernames, hostnames, machine IDs, hardware serials, MAC addresses, or IP addresses.
- Process arguments, environments, working directories, or open files.
- Shell history, browser data, SSH keys, tokens, credentials, cookies, or arbitrary file content.
- Raw journal entries and raw command output.

## Reporting bugs

Do not attach the SQLite database or a raw snapshot to a public issue. Reproduce problems with a sanitized fixture under `fixtures/` instead.
