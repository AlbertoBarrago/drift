use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
};

use chrono::Utc;
use drift_domain::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CollectError {
    #[error("command failed: {program}")]
    CommandFailed { program: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub trait CommandRunner {
    fn run(&self, program: &str, arguments: &[&str]) -> Result<String, CollectError>;
}

pub trait SystemProbe: CommandRunner {
    fn file_exists(&self, path: &Path) -> bool;
    fn read_file(&self, path: &Path) -> Result<Vec<u8>, CollectError>;
    fn read_tail(&self, path: &Path, max_bytes: u64) -> Result<Vec<u8>, CollectError>;
    fn home_dir(&self) -> Option<PathBuf>;
}

pub trait SnapshotExtension {
    fn collect(&self, probe: &dyn SystemProbe, facts: &mut SnapshotFacts) -> CollectorReport;
}

pub struct SystemCommandRunner;
impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &str, arguments: &[&str]) -> Result<String, CollectError> {
        let output = Command::new(program)
            .args(arguments)
            .env("LC_ALL", "C")
            .output()?;
        if !output.status.success() {
            return Err(CollectError::CommandFailed {
                program: program.into(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl SystemProbe for SystemCommandRunner {
    fn file_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>, CollectError> {
        Ok(fs::read(path)?)
    }

    fn read_tail(&self, path: &Path, max_bytes: u64) -> Result<Vec<u8>, CollectError> {
        let mut file = fs::File::open(path)?;
        let start = file.metadata()?.len().saturating_sub(max_bytes);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn home_dir(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

pub fn collect_snapshot(
    probe: &dyn SystemProbe,
    host_scope_id: String,
    extensions: &[&dyn SnapshotExtension],
) -> SystemSnapshot {
    let started_at = Utc::now();
    let mut facts = SnapshotFacts::default();
    let mut reports = Vec::new();
    facts.kernel_release = probe
        .run("uname", &["-r"])
        .ok()
        .map(|value| value.trim().to_owned());
    reports.push(report("system", facts.kernel_release.is_some()));
    collect_pacman(probe, &mut facts, &mut reports);
    collect_pacman_transactions(probe, &mut facts, &mut reports);
    collect_systemd(probe, &mut facts, &mut reports);
    collect_processes(probe, &mut facts, &mut reports);
    collect_graphics(probe, &mut facts, &mut reports);
    collect_network(probe, &mut facts, &mut reports);
    collect_ports(probe, &mut facts, &mut reports);
    collect_docker(probe, &mut facts, &mut reports);
    collect_runtimes(probe, &mut facts, &mut reports);
    collect_config(probe, &mut facts, &mut reports);
    for extension in extensions {
        reports.push(extension.collect(probe, &mut facts));
    }
    let completed_at = Utc::now();
    SystemSnapshot::new(host_scope_id, started_at, completed_at, reports, facts)
}

fn collect_pacman(
    runner: &dyn CommandRunner,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    let packages = runner.run("pacman", &["-Q"]);
    let Ok(packages) = packages else {
        reports.push(unavailable("pacman"));
        return;
    };
    let explicit: std::collections::BTreeSet<String> = runner
        .run("pacman", &["-Qeq"])
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    for line in packages.lines() {
        let Some((name, version)) = line.split_once(' ') else {
            continue;
        };
        facts.packages.insert(
            name.to_owned(),
            PackageFact {
                version: version.to_owned(),
                install_reason: if explicit.contains(name) {
                    InstallReason::Explicit
                } else {
                    InstallReason::Dependency
                },
                repository: None,
            },
        );
    }
    reports.push(report("pacman", true));
}

fn collect_pacman_transactions(
    probe: &dyn SystemProbe,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    let Ok(bytes) = probe.read_tail(Path::new("/var/log/pacman.log"), 128 * 1024) else {
        reports.push(unavailable("pacman-transactions"));
        return;
    };
    let text = String::from_utf8_lossy(&bytes);
    facts.package_transactions = text
        .lines()
        .filter_map(parse_pacman_transaction)
        .rev()
        .take(200)
        .collect::<Vec<_>>();
    facts.package_transactions.reverse();
    reports.push(report("pacman-transactions", true));
}

fn parse_pacman_transaction(line: &str) -> Option<PackageTransactionFact> {
    let (timestamp, message) = line.split_once("] [ALPM] ")?;
    let timestamp = timestamp.strip_prefix('[')?.to_owned();
    if let Some(value) = message
        .strip_prefix("installed ")
        .and_then(|value| value.strip_suffix(')'))
    {
        let (package, version) = value.rsplit_once(" (")?;
        return Some(PackageTransactionFact {
            timestamp,
            operation: PackageTransactionOperation::Installed,
            package: package.to_owned(),
            from_version: None,
            to_version: Some(version.to_owned()),
        });
    }
    if let Some(value) = message
        .strip_prefix("removed ")
        .and_then(|value| value.strip_suffix(')'))
    {
        let (package, version) = value.rsplit_once(" (")?;
        return Some(PackageTransactionFact {
            timestamp,
            operation: PackageTransactionOperation::Removed,
            package: package.to_owned(),
            from_version: Some(version.to_owned()),
            to_version: None,
        });
    }
    let value = message.strip_prefix("upgraded ")?.strip_suffix(')')?;
    let (package, versions) = value.rsplit_once(" (")?;
    let (from_version, to_version) = versions.split_once(" -> ")?;
    Some(PackageTransactionFact {
        timestamp,
        operation: PackageTransactionOperation::Upgraded,
        package: package.to_owned(),
        from_version: Some(from_version.to_owned()),
        to_version: Some(to_version.to_owned()),
    })
}

fn collect_systemd(
    runner: &dyn CommandRunner,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    collect_systemd_scope(runner, ServiceScope::System, false, facts, reports);
    collect_systemd_scope(runner, ServiceScope::User, true, facts, reports);
}

fn collect_systemd_scope(
    runner: &dyn CommandRunner,
    scope: ServiceScope,
    user: bool,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    let mut unit_file_args = Vec::new();
    if user {
        unit_file_args.push("--user");
    }
    unit_file_args.extend([
        "list-unit-files",
        "--type=service",
        "--no-legend",
        "--no-pager",
    ]);
    let mut runtime_args = Vec::new();
    if user {
        runtime_args.push("--user");
    }
    runtime_args.extend([
        "list-units",
        "--type=service",
        "--all",
        "--no-legend",
        "--no-pager",
        "--plain",
    ]);
    let unit_files = runner.run("systemctl", &unit_file_args);
    let runtime_units = runner.run("systemctl", &runtime_args);
    let (Ok(unit_files), Ok(runtime_units)) = (unit_files, runtime_units) else {
        reports.push(unavailable(if user {
            "systemd-user"
        } else {
            "systemd-system"
        }));
        return;
    };
    for line in unit_files.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(state)) = (fields.next(), fields.next()) else {
            continue;
        };
        facts.services.insert(
            ServiceKey {
                scope: scope.clone(),
                name: name.into(),
            },
            ServiceFact {
                enablement: enablement(state),
                active_state: UnitState::Unknown,
                sub_state: None,
            },
        );
    }
    for line in runtime_units.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(_load), Some(active), Some(sub_state)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let key = ServiceKey {
            scope: scope.clone(),
            name: name.into(),
        };
        let service = facts.services.entry(key).or_insert(ServiceFact {
            enablement: Enablement::Unknown,
            active_state: UnitState::Unknown,
            sub_state: None,
        });
        service.active_state = unit_state(active);
        service.sub_state = Some(sub_state.into());
    }
    reports.push(report(
        if user {
            "systemd-user"
        } else {
            "systemd-system"
        },
        true,
    ));
}

fn collect_processes(
    probe: &dyn SystemProbe,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    let Ok(output) = probe.run("ps", &["-eo", "comm="]) else {
        reports.push(unavailable("processes"));
        return;
    };
    for name in output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        facts
            .processes
            .entry(name.into())
            .and_modify(|fact| fact.count += 1)
            .or_insert(ProcessFact { count: 1 });
    }
    reports.push(report("processes", true));
}

fn collect_graphics(
    probe: &dyn SystemProbe,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    let Ok(output) = probe.run("lspci", &["-nnk"]) else {
        reports.push(unavailable("graphics"));
        return;
    };
    let mut device_index = 0_u32;
    let mut current_device = None;
    for line in output.lines() {
        let trimmed = line.trim_start();
        if line.contains("VGA compatible controller")
            || line.contains("3D controller")
            || line.contains("Display controller")
        {
            current_device = Some(format!("gpu{device_index}"));
            device_index += 1;
            continue;
        }
        if let (Some(device), Some(driver)) = (
            current_device.as_ref(),
            trimmed.strip_prefix("Kernel driver in use: "),
        ) {
            facts.graphics.insert(
                device.clone(),
                GraphicsFact {
                    driver: Some(driver.to_owned()),
                },
            );
        }
    }
    reports.push(report("graphics", true));
}

fn collect_network(
    probe: &dyn SystemProbe,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    let Ok(output) = probe.run("ip", &["-o", "link", "show"]) else {
        reports.push(unavailable("network"));
        return;
    };
    for line in output.lines() {
        let mut fields = line.split_whitespace();
        let (Some(_index), Some(raw_name)) = (fields.next(), fields.next()) else {
            continue;
        };
        let name = raw_name
            .trim_end_matches(':')
            .split('@')
            .next()
            .unwrap_or_default();
        let state = line
            .split(" state ")
            .nth(1)
            .and_then(|value| value.split_whitespace().next())
            .unwrap_or("unknown");
        if !name.is_empty() {
            facts.network_interfaces.insert(
                name.into(),
                NetworkInterfaceFact {
                    operational_state: state.into(),
                },
            );
        }
    }
    reports.push(report("network", true));
}

fn collect_ports(
    probe: &dyn SystemProbe,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    let Ok(output) = probe.run("ss", &["-H", "-ltnu"]) else {
        reports.push(unavailable("ports"));
        return;
    };
    for line in output.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let Some(protocol) = fields.first().and_then(|value| match *value {
            "tcp" => Some(TransportProtocol::Tcp),
            "udp" => Some(TransportProtocol::Udp),
            _ => None,
        }) else {
            continue;
        };
        let Some(address) = fields
            .iter()
            .find(|field| field.contains(':') || field.starts_with('*'))
        else {
            continue;
        };
        let Some(raw_port) = address.rsplit(':').next() else {
            continue;
        };
        let Ok(port) = raw_port.parse() else { continue };
        let address_scope = if address.starts_with("127.") || address.starts_with("[::1]") {
            AddressScope::Loopback
        } else if address.starts_with("0.0.0.0")
            || address.starts_with("[::]")
            || address.starts_with("*")
        {
            AddressScope::Wildcard
        } else {
            AddressScope::Specific
        };
        facts.listening_ports.insert(ListeningPortFact {
            protocol,
            port,
            address_scope,
        });
    }
    reports.push(report("ports", true));
}

fn collect_docker(
    probe: &dyn SystemProbe,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    let version = probe
        .run("docker", &["version", "--format", "{{.Client.Version}}"])
        .ok()
        .map(|value| value.trim().to_owned());
    let available = probe.file_exists(Path::new("/run/docker.sock"))
        && probe
            .run("docker", &["info", "--format", "{{.ServerVersion}}"])
            .is_ok();
    facts
        .runtimes
        .insert("docker".into(), RuntimeFact { version, available });
    reports.push(report("docker", true));
}

fn collect_runtimes(
    probe: &dyn SystemProbe,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    for (name, program, arguments) in [
        ("git", "git", vec!["--version"]),
        ("node", "node", vec!["--version"]),
        ("python", "python", vec!["--version"]),
        ("rust", "rustc", vec!["--version"]),
        ("neovim", "nvim", vec!["--version"]),
    ] {
        let version = probe.run(program, &arguments).ok().and_then(|output| {
            output
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
        });
        facts.runtimes.insert(
            name.into(),
            RuntimeFact {
                available: version.is_some(),
                version,
            },
        );
    }
    reports.push(report("runtimes", true));
}

fn collect_config(
    probe: &dyn SystemProbe,
    facts: &mut SnapshotFacts,
    reports: &mut Vec<CollectorReport>,
) {
    for (id, path) in config_targets(probe.home_dir()) {
        let value = probe.read_file(&path).ok();
        facts.config.insert(
            id.into(),
            ConfigFileFact {
                exists: value.is_some(),
                digest: value
                    .as_ref()
                    .map(|bytes| blake3::hash(bytes).to_hex().to_string()),
                size: value.map(|bytes| bytes.len() as u64),
            },
        );
    }
    reports.push(report("config", true));
}

fn config_targets(home: Option<PathBuf>) -> Vec<(&'static str, PathBuf)> {
    let home = home.unwrap_or_default();
    vec![
        ("hyprland/main", home.join(".config/hypr/hyprland.conf")),
        ("waybar/config", home.join(".config/waybar/config")),
        (
            "waybar/config.jsonc",
            home.join(".config/waybar/config.jsonc"),
        ),
        ("shell/zsh", home.join(".zshrc")),
        ("shell/bash", home.join(".bashrc")),
        ("shell/fish", home.join(".config/fish/config.fish")),
        ("git/config", home.join(".gitconfig")),
    ]
}

fn report(name: &str, complete: bool) -> CollectorReport {
    CollectorReport {
        name: name.into(),
        version: "1".into(),
        outcome: if complete {
            CollectorOutcome::Complete
        } else {
            CollectorOutcome::Partial
        },
        detail: None,
    }
}
fn unavailable(name: &str) -> CollectorReport {
    CollectorReport {
        name: name.into(),
        version: "1".into(),
        outcome: CollectorOutcome::Unavailable,
        detail: None,
    }
}
fn enablement(value: &str) -> Enablement {
    match value {
        "enabled" | "enabled-runtime" => Enablement::Enabled,
        "disabled" => Enablement::Disabled,
        "static" => Enablement::Static,
        "masked" => Enablement::Masked,
        "indirect" => Enablement::Indirect,
        _ => Enablement::Unknown,
    }
}
fn unit_state(value: &str) -> UnitState {
    match value {
        "active" => UnitState::Active,
        "inactive" => UnitState::Inactive,
        "failed" => UnitState::Failed,
        "activating" => UnitState::Activating,
        "deactivating" => UnitState::Deactivating,
        _ => UnitState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use super::*;

    struct FakeProbe {
        commands: BTreeMap<String, String>,
        files: BTreeMap<PathBuf, Vec<u8>>,
        home: PathBuf,
    }

    impl FakeProbe {
        fn command_key(program: &str, arguments: &[&str]) -> String {
            format!("{program} {}", arguments.join(" "))
        }
    }

    impl CommandRunner for FakeProbe {
        fn run(&self, program: &str, arguments: &[&str]) -> Result<String, CollectError> {
            self.commands
                .get(&Self::command_key(program, arguments))
                .cloned()
                .ok_or_else(|| CollectError::CommandFailed {
                    program: program.into(),
                })
        }
    }

    impl SystemProbe for FakeProbe {
        fn file_exists(&self, path: &Path) -> bool {
            self.files.contains_key(path)
        }

        fn read_file(&self, path: &Path) -> Result<Vec<u8>, CollectError> {
            self.files.get(path).cloned().ok_or_else(|| {
                CollectError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "fixture file",
                ))
            })
        }

        fn read_tail(&self, path: &Path, _max_bytes: u64) -> Result<Vec<u8>, CollectError> {
            self.read_file(path)
        }

        fn home_dir(&self) -> Option<PathBuf> {
            Some(self.home.clone())
        }
    }

    #[test]
    fn collects_a_deterministic_arch_fixture() {
        let home = PathBuf::from("/fixture/home");
        let commands = BTreeMap::from([
            ("uname -r".into(), "6.16.0-arch1-1\n".into()),
            (
                "pacman -Q".into(),
                "docker 28.0.0-1\nlinux 6.16.0.arch1-1\n".into(),
            ),
            ("pacman -Qeq".into(), "docker\n".into()),
            (
                "systemctl list-unit-files --type=service --no-legend --no-pager".into(),
                "docker.service enabled enabled\nssh.service disabled disabled\n".into(),
            ),
            (
                "systemctl list-units --type=service --all --no-legend --no-pager --plain".into(),
                "docker.service loaded active running Docker Application Container Engine\n".into(),
            ),
            (
                "systemctl --user list-unit-files --type=service --no-legend --no-pager".into(),
                "waybar.service enabled enabled\n".into(),
            ),
            (
                "systemctl --user list-units --type=service --all --no-legend --no-pager --plain".into(),
                "waybar.service loaded active running Waybar\n".into(),
            ),
            ("ps -eo comm=".into(), "systemd\ndockerd\ndockerd\nwaybar\n".into()),
            (
                "lspci -nnk".into(),
                "01:00.0 VGA compatible controller [0300]: NVIDIA Corporation Device [10de:28a0]\n\tKernel driver in use: nvidia\n".into(),
            ),
            (
                "ip -o link show".into(),
                "1: lo: <LOOPBACK,UP> mtu 65536 state UNKNOWN mode DEFAULT\n2: wlan0: <BROADCAST,UP> mtu 1500 state UP mode DEFAULT\n".into(),
            ),
            (
                "ss -H -ltnu".into(),
                "tcp LISTEN 0 4096 127.0.0.1:631 0.0.0.0:*\nudp UNCONN 0 0 0.0.0.0:5353 0.0.0.0:*\n".into(),
            ),
            (
                "docker version --format {{.Client.Version}}".into(),
                "28.0.0\n".into(),
            ),
            ("git --version".into(), "git version 2.49.0\n".into()),
            ("node --version".into(), "v22.15.0\n".into()),
            ("python --version".into(), "Python 3.13.2\n".into()),
            ("rustc --version".into(), "rustc 1.86.0\n".into()),
            ("nvim --version".into(), "NVIM v0.11.0\n".into()),
            (
                "docker info --format {{.ServerVersion}}".into(),
                "28.0.0\n".into(),
            ),
        ]);
        let files = BTreeMap::from([
            (PathBuf::from("/run/docker.sock"), Vec::new()),
            (
                home.join(".config/hypr/hyprland.conf"),
                b"monitor=,preferred,auto,1".to_vec(),
            ),
            (
                PathBuf::from("/var/log/pacman.log"),
                b"[2026-09-15T10:00:00+0000] [ALPM] upgraded mesa (26.0.1-1 -> 26.0.2-1)\n[2026-09-15T10:01:00+0000] [ALPM] installed docker (28.0.0-1)\n".to_vec(),
            ),
        ]);
        let probe = FakeProbe {
            commands,
            files,
            home,
        };

        let snapshot = collect_snapshot(&probe, "fixture-host".into(), &[]);
        let docker = ServiceKey {
            scope: ServiceScope::System,
            name: "docker.service".into(),
        };

        assert_eq!(
            snapshot.facts.kernel_release.as_deref(),
            Some("6.16.0-arch1-1")
        );
        assert_eq!(
            snapshot.facts.packages["docker"].install_reason,
            InstallReason::Explicit
        );
        assert_eq!(
            snapshot.facts.services[&docker].enablement,
            Enablement::Enabled
        );
        assert_eq!(snapshot.facts.package_transactions.len(), 2);
        assert!(snapshot.facts.services.contains_key(&ServiceKey {
            scope: ServiceScope::User,
            name: "waybar.service".into(),
        }));
        assert!(snapshot.facts.runtimes["docker"].available);
        assert_eq!(snapshot.facts.processes["dockerd"].count, 2);
        assert_eq!(
            snapshot.facts.graphics["gpu0"].driver.as_deref(),
            Some("nvidia")
        );
        assert_eq!(
            snapshot.facts.network_interfaces["wlan0"].operational_state,
            "UP"
        );
        assert!(snapshot.facts.listening_ports.contains(&ListeningPortFact {
            protocol: TransportProtocol::Tcp,
            port: 631,
            address_scope: AddressScope::Loopback,
        }));
        assert_eq!(
            snapshot.facts.runtimes["node"].version.as_deref(),
            Some("v22.15.0")
        );
        assert!(snapshot.facts.config["hyprland/main"].digest.is_some());
    }
}
