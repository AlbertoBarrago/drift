use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub schema_version: u32,
    pub id: String,
    pub host_scope_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub collectors: Vec<CollectorReport>,
    pub facts: SnapshotFacts,
    pub content_hash: String,
}

impl SystemSnapshot {
    pub fn new(
        host_scope_id: String,
        started_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
        collectors: Vec<CollectorReport>,
        facts: SnapshotFacts,
    ) -> Self {
        let mut snapshot = Self {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            id: String::new(),
            host_scope_id,
            started_at,
            completed_at,
            collectors,
            facts,
            content_hash: String::new(),
        };
        snapshot.content_hash = snapshot.compute_hash();
        snapshot.id = snapshot.content_hash[..16].to_owned();
        snapshot
    }

    fn compute_hash(&self) -> String {
        let services: BTreeMap<String, &ServiceFact> = self
            .facts
            .services
            .iter()
            .map(|(key, fact)| {
                let scope = match &key.scope {
                    ServiceScope::System => "system",
                    ServiceScope::User => "user",
                };
                (format!("{scope}:{}", key.name), fact)
            })
            .collect();
        let value = serde_json::to_vec(&(
            self.schema_version,
            &self.host_scope_id,
            self.started_at,
            self.completed_at,
            &self.collectors,
            &self.facts.kernel_release,
            &self.facts.packages,
            &self.facts.package_transactions,
            services,
            &self.facts.processes,
            &self.facts.graphics,
            &self.facts.network_interfaces,
            &self.facts.listening_ports,
            &self.facts.waybar_referenced_interfaces,
            &self.facts.config,
            &self.facts.runtimes,
        ))
        .expect("snapshot facts must serialize");
        blake3::hash(&value).to_hex().to_string()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SnapshotFacts {
    pub kernel_release: Option<String>,
    pub packages: BTreeMap<String, PackageFact>,
    pub package_transactions: Vec<PackageTransactionFact>,
    pub services: BTreeMap<ServiceKey, ServiceFact>,
    pub processes: BTreeMap<String, ProcessFact>,
    pub graphics: BTreeMap<String, GraphicsFact>,
    pub network_interfaces: BTreeMap<String, NetworkInterfaceFact>,
    pub listening_ports: BTreeSet<ListeningPortFact>,
    pub waybar_referenced_interfaces: BTreeSet<String>,
    pub config: BTreeMap<String, ConfigFileFact>,
    pub runtimes: BTreeMap<String, RuntimeFact>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PackageFact {
    pub version: String,
    pub install_reason: InstallReason,
    pub repository: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PackageTransactionFact {
    pub timestamp: String,
    pub operation: PackageTransactionOperation,
    pub package: String,
    pub from_version: Option<String>,
    pub to_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PackageTransactionOperation {
    Installed,
    Upgraded,
    Removed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum InstallReason {
    Explicit,
    Dependency,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ServiceKey {
    pub scope: ServiceScope,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum ServiceScope {
    System,
    User,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServiceFact {
    pub enablement: Enablement,
    pub active_state: UnitState,
    pub sub_state: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessFact {
    pub count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GraphicsFact {
    pub driver: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkInterfaceFact {
    pub operational_state: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ListeningPortFact {
    pub protocol: TransportProtocol,
    pub port: u16,
    pub address_scope: AddressScope,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum TransportProtocol {
    Tcp,
    Udp,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum AddressScope {
    Loopback,
    Wildcard,
    Specific,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Enablement {
    Enabled,
    Disabled,
    Static,
    Masked,
    Indirect,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UnitState {
    Active,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigFileFact {
    pub exists: bool,
    pub digest: Option<String>,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeFact {
    pub version: Option<String>,
    pub available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CollectorReport {
    pub name: String,
    pub version: String,
    pub outcome: CollectorOutcome,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CollectorOutcome {
    Complete,
    Partial,
    Unavailable,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateDiff {
    pub from: String,
    pub to: String,
    pub changes: Vec<StateChange>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StateChange {
    KernelVersionChanged {
        from: Option<String>,
        to: Option<String>,
    },
    PackageInstalled {
        name: String,
        package: PackageFact,
    },
    PackageRemoved {
        name: String,
        package: PackageFact,
    },
    PackageVersionChanged {
        name: String,
        from: String,
        to: String,
    },
    ServiceEnablementChanged {
        service: ServiceKey,
        from: Enablement,
        to: Enablement,
    },
    ServiceRuntimeChanged {
        service: ServiceKey,
        from: UnitState,
        to: UnitState,
    },
    ProcessCountChanged {
        executable: String,
        from: u32,
        to: u32,
    },
    GraphicsDriverChanged {
        device: String,
        from: Option<String>,
        to: Option<String>,
    },
    PortOpened {
        port: ListeningPortFact,
    },
    PortClosed {
        port: ListeningPortFact,
    },
    NetworkInterfaceAdded {
        name: String,
        interface: NetworkInterfaceFact,
    },
    NetworkInterfaceRemoved {
        name: String,
        interface: NetworkInterfaceFact,
    },
    NetworkInterfaceStateChanged {
        name: String,
        from: String,
        to: String,
    },
    ConfigChanged {
        target: String,
    },
    RuntimeVersionChanged {
        runtime: String,
        from: Option<String>,
        to: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Diagnosis {
    pub id: String,
    pub rule_id: String,
    pub confidence: Confidence,
    pub summary: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub edges: Vec<DependencyEdge>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub from: DependencyNode,
    pub relation: DependencyRelation,
    pub to: DependencyNode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DependencyNode {
    Runtime(String),
    Service(ServiceKey),
    Package(String),
    Config(String),
    NetworkInterface(String),
}

impl DependencyNode {
    pub fn label(&self) -> String {
        match self {
            Self::Runtime(name) => format!("runtime:{name}"),
            Self::Service(service) => {
                let scope = match &service.scope {
                    ServiceScope::System => "system",
                    ServiceScope::User => "user",
                };
                format!("service:{scope}:{}", service.name)
            }
            Self::Package(name) => format!("package:{name}"),
            Self::Config(name) => format!("config:{name}"),
            Self::NetworkInterface(name) => format!("network-interface:{name}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DependencyRelation {
    Requires,
    ManagedBy,
    References,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: String,
    pub detail: String,
}
