use std::{fs, path::PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use drift_collect::{SnapshotExtension, SystemCommandRunner, collect_snapshot};
use drift_domain::{
    DependencyGraph, Diagnosis, PackageTransactionOperation, StateChange, SystemSnapshot,
};
use drift_engine::{DiagnosisProfile, dependency_graph, diagnose, diff};
use drift_profile_arch::ArchProfile;
use drift_profile_omarchy::OmarchyProfile;
use drift_store::SnapshotStore;
use thiserror::Error;

#[derive(Parser)]
#[command(name = "drift", about = "Local deterministic workstation diagnostics")]
struct Cli {
    #[arg(long, value_enum, default_value_t = ProfileSelection::Arch)]
    profile: ProfileSelection,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, ValueEnum)]
enum ProfileSelection {
    Arch,
    Omarchy,
}

#[derive(Subcommand)]
enum Command {
    Snapshot,
    Snapshots,
    MarkGood,
    Diff {
        from: String,
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Status,
    Diagnose {
        #[arg(long)]
        no_store: bool,
    },
    Explain {
        diagnosis_id: String,
    },
    Timeline,
    Inspect {
        target: Option<String>,
    },
}

#[derive(Debug, Error)]
enum AppError {
    #[error(transparent)]
    Store(#[from] drift_store::StoreError),
    #[error("snapshot not found: {0}")]
    SnapshotNotFound(String),
    #[error("no known-good snapshot, run `drift mark-good` first")]
    NoKnownGoodSnapshot,
    #[error("diagnosis not found: {0}")]
    DiagnosisNotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn main() -> Result<(), AppError> {
    let Cli { profile, command } = Cli::parse();
    let state_dir = state_dir()?;
    fs::create_dir_all(&state_dir)?;
    let store = SnapshotStore::open(&state_dir.join("snapshots.sqlite3"))?;
    match command {
        Command::Snapshot => {
            let snapshot = capture(&state_dir, &profile);
            store.save(&snapshot)?;
            println!("Snapshot {} created.", snapshot.id);
        }
        Command::Snapshots => {
            for (id, timestamp, _) in store.list()? {
                println!("{}  {}", id, timestamp.to_rfc3339());
            }
        }
        Command::MarkGood => {
            let snapshot = capture(&state_dir, &profile);
            store.save(&snapshot)?;
            store.tag("known-good", &snapshot.id)?;
            println!("Snapshot {} marked known-good.", snapshot.id);
        }
        Command::Diff { from, to, json } => {
            let before = snapshot(&store, &from)?;
            let after = match to {
                Some(id) => snapshot(&store, &id)?,
                None => store
                    .latest()?
                    .ok_or_else(|| AppError::SnapshotNotFound("latest".into()))?,
            };
            let state_diff = diff(&before, &after);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&state_diff).expect("diff serializes")
                );
            } else {
                render_diff(&state_diff);
            }
        }
        Command::Status => {
            let current = capture(&state_dir, &profile);
            for report in &current.collectors {
                println!("{:12} {:?}", report.name, report.outcome);
            }
            if current
                .facts
                .services
                .values()
                .any(|service| service.active_state == drift_domain::UnitState::Failed)
            {
                println!("Failed systemd units detected.");
            }
        }
        Command::Diagnose { no_store } => {
            let good = store
                .tagged("known-good")?
                .ok_or(AppError::NoKnownGoodSnapshot)?;
            let current = capture(&state_dir, &profile);
            if !no_store {
                store.save(&current)?;
            }
            let state_diff = diff(&good, &current);
            let diagnoses = all_diagnoses(&good, &current, &state_diff);
            if diagnoses.is_empty() {
                println!("No high-confidence diagnosis found.");
            } else {
                for diagnosis in &diagnoses {
                    render_diagnosis(diagnosis);
                }
            }
        }
        Command::Explain { diagnosis_id } => {
            render_diagnosis(&find_diagnosis(&store, &diagnosis_id)?);
        }
        Command::Timeline => render_timeline(&store)?,
        Command::Inspect { target } => {
            let current = capture(&state_dir, &profile);
            render_graph(&all_graph(&current), target.as_deref());
        }
    }
    Ok(())
}

fn find_diagnosis(store: &SnapshotStore, diagnosis_id: &str) -> Result<Diagnosis, AppError> {
    let mut parts = diagnosis_id.rsplit('@');
    let (Some(current_id), Some(baseline_id), Some(_rule_id)) =
        (parts.next(), parts.next(), parts.next())
    else {
        return Err(AppError::DiagnosisNotFound(diagnosis_id.into()));
    };
    let baseline = snapshot(store, baseline_id)?;
    let current = snapshot(store, current_id)?;
    all_diagnoses(&baseline, &current, &diff(&baseline, &current))
        .into_iter()
        .find(|diagnosis| diagnosis.id == diagnosis_id)
        .ok_or_else(|| AppError::DiagnosisNotFound(diagnosis_id.into()))
}

fn render_timeline(store: &SnapshotStore) -> Result<(), AppError> {
    let known_good_id = store.tagged("known-good")?.map(|snapshot| snapshot.id);
    let mut events = std::collections::BTreeSet::new();
    for snapshot in store.all()? {
        let marker = if known_good_id.as_deref() == Some(snapshot.id.as_str()) {
            " known-good"
        } else {
            ""
        };
        events.insert(format!(
            "{}  snapshot {}{}",
            snapshot.completed_at.to_rfc3339(),
            snapshot.id,
            marker
        ));
        for transaction in snapshot.facts.package_transactions {
            let operation = match transaction.operation {
                PackageTransactionOperation::Installed => "installed",
                PackageTransactionOperation::Upgraded => "upgraded",
                PackageTransactionOperation::Removed => "removed",
            };
            events.insert(format!(
                "{}  package {} {}{}",
                transaction.timestamp,
                operation,
                transaction.package,
                transaction
                    .to_version
                    .as_ref()
                    .map(|version| format!(" -> {version}"))
                    .unwrap_or_default()
            ));
        }
    }
    for event in events {
        println!("{event}");
    }
    Ok(())
}

fn state_dir() -> Result<PathBuf, std::io::Error> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(base.join("drift"))
}

fn capture(state_dir: &std::path::Path, profile: &ProfileSelection) -> SystemSnapshot {
    let host_scope_path = state_dir.join("host-scope-id");
    let host_scope_id = fs::read_to_string(&host_scope_path).unwrap_or_else(|_| {
        let mut bytes = [0_u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes))
            .expect("secure random source is available");
        let generated = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let _ = fs::write(&host_scope_path, &generated);
        generated
    });
    let omarchy = OmarchyProfile;
    let extensions: Vec<&dyn SnapshotExtension> = match profile {
        ProfileSelection::Arch => Vec::new(),
        ProfileSelection::Omarchy => vec![&omarchy],
    };
    collect_snapshot(
        &SystemCommandRunner,
        host_scope_id.trim().to_owned(),
        &extensions,
    )
}

fn all_diagnoses(
    known_good: &SystemSnapshot,
    current: &SystemSnapshot,
    state_diff: &drift_domain::StateDiff,
) -> Vec<Diagnosis> {
    let mut diagnoses = diagnose(known_good, current, state_diff);
    let arch = ArchProfile;
    let omarchy = OmarchyProfile;
    for profile in [
        &arch as &dyn DiagnosisProfile,
        &omarchy as &dyn DiagnosisProfile,
    ] {
        diagnoses.extend(profile.diagnose(known_good, current, state_diff));
    }
    diagnoses
}

fn all_graph(snapshot: &SystemSnapshot) -> DependencyGraph {
    let mut graph = dependency_graph(snapshot);
    let arch = ArchProfile;
    let omarchy = OmarchyProfile;
    for profile in [
        &arch as &dyn DiagnosisProfile,
        &omarchy as &dyn DiagnosisProfile,
    ] {
        graph.edges.extend(profile.graph_edges(snapshot));
    }
    graph
}

fn render_graph(graph: &DependencyGraph, target: Option<&str>) {
    for edge in graph.edges.iter().filter(|edge| {
        target.is_none_or(|target| {
            edge.from.label().contains(target) || edge.to.label().contains(target)
        })
    }) {
        println!(
            "{} --{:?}--> {}",
            edge.from.label(),
            edge.relation,
            edge.to.label()
        );
    }
}

fn snapshot(store: &SnapshotStore, id: &str) -> Result<SystemSnapshot, AppError> {
    store
        .get(id)?
        .ok_or_else(|| AppError::SnapshotNotFound(id.into()))
}

fn render_diff(state_diff: &drift_domain::StateDiff) {
    if state_diff.changes.is_empty() {
        println!("No semantic changes detected.");
        return;
    }
    println!("Changes detected:");
    for change in &state_diff.changes {
        match change {
            StateChange::KernelVersionChanged { from, to } => {
                println!("  kernel: {:?} -> {:?}", from, to)
            }
            StateChange::PackageInstalled { name, package } => {
                println!("  package installed: {name} {}", package.version)
            }
            StateChange::PackageRemoved { name, package } => {
                println!("  package removed: {name} {}", package.version)
            }
            StateChange::PackageVersionChanged { name, from, to } => {
                println!("  package: {name} {from} -> {to}")
            }
            StateChange::ServiceEnablementChanged { service, from, to } => {
                println!("  service: {} {:?} -> {:?}", service.name, from, to)
            }
            StateChange::ServiceRuntimeChanged { service, from, to } => {
                println!("  service: {} {:?} -> {:?}", service.name, from, to)
            }
            StateChange::ProcessCountChanged {
                executable,
                from,
                to,
            } => {
                println!("  process: {executable} count {from} -> {to}")
            }
            StateChange::GraphicsDriverChanged { device, from, to } => {
                println!("  graphics driver: {device} {:?} -> {:?}", from, to)
            }
            StateChange::PortOpened { port } => println!(
                "  port opened: {:?}/{} ({:?})",
                port.protocol, port.port, port.address_scope
            ),
            StateChange::PortClosed { port } => println!(
                "  port closed: {:?}/{} ({:?})",
                port.protocol, port.port, port.address_scope
            ),
            StateChange::NetworkInterfaceAdded { name, interface } => {
                println!(
                    "  interface added: {name} ({})",
                    interface.operational_state
                )
            }
            StateChange::NetworkInterfaceRemoved { name, interface } => {
                println!(
                    "  interface removed: {name} ({})",
                    interface.operational_state
                )
            }
            StateChange::NetworkInterfaceStateChanged { name, from, to } => {
                println!("  interface: {name} {from} -> {to}")
            }
            StateChange::ConfigChanged { target } => println!("  config changed: {target}"),
            StateChange::RuntimeVersionChanged { runtime, from, to } => {
                println!("  runtime: {runtime} {:?} -> {:?}", from, to)
            }
        }
    }
}

fn render_diagnosis(diagnosis: &Diagnosis) {
    println!(
        "{:?} confidence: {}",
        diagnosis.confidence, diagnosis.summary
    );
    for evidence in &diagnosis.evidence {
        println!("  {}: {}", evidence.kind, evidence.detail);
    }
}
