use drift_domain::*;

pub trait DiagnosisProfile {
    fn id(&self) -> &'static str;
    fn diagnose(
        &self,
        known_good: &SystemSnapshot,
        current: &SystemSnapshot,
        state_diff: &StateDiff,
    ) -> Vec<Diagnosis>;

    fn graph_edges(&self, _snapshot: &SystemSnapshot) -> Vec<DependencyEdge> {
        Vec::new()
    }
}

pub fn dependency_graph(snapshot: &SystemSnapshot) -> DependencyGraph {
    let docker_service = ServiceKey {
        scope: ServiceScope::System,
        name: "docker.service".into(),
    };
    let mut edges = Vec::new();
    if snapshot.facts.runtimes.contains_key("docker") {
        edges.push(DependencyEdge {
            from: DependencyNode::Runtime("docker".into()),
            relation: DependencyRelation::Requires,
            to: DependencyNode::Service(docker_service.clone()),
        });
    }
    if snapshot.facts.services.contains_key(&docker_service)
        && snapshot.facts.packages.contains_key("docker")
    {
        edges.push(DependencyEdge {
            from: DependencyNode::Service(docker_service),
            relation: DependencyRelation::ManagedBy,
            to: DependencyNode::Package("docker".into()),
        });
    }
    DependencyGraph { edges }
}

pub fn diff(before: &SystemSnapshot, after: &SystemSnapshot) -> StateDiff {
    let mut changes = Vec::new();
    if before.facts.kernel_release != after.facts.kernel_release {
        changes.push(StateChange::KernelVersionChanged {
            from: before.facts.kernel_release.clone(),
            to: after.facts.kernel_release.clone(),
        });
    }
    for (name, package) in &after.facts.packages {
        match before.facts.packages.get(name) {
            None => changes.push(StateChange::PackageInstalled {
                name: name.clone(),
                package: package.clone(),
            }),
            Some(previous) if previous.version != package.version => {
                changes.push(StateChange::PackageVersionChanged {
                    name: name.clone(),
                    from: previous.version.clone(),
                    to: package.version.clone(),
                })
            }
            _ => {}
        }
    }
    for (name, package) in &before.facts.packages {
        if !after.facts.packages.contains_key(name) {
            changes.push(StateChange::PackageRemoved {
                name: name.clone(),
                package: package.clone(),
            });
        }
    }
    for (key, service) in &after.facts.services {
        if let Some(previous) = before.facts.services.get(key) {
            if previous.enablement != service.enablement {
                changes.push(StateChange::ServiceEnablementChanged {
                    service: key.clone(),
                    from: previous.enablement.clone(),
                    to: service.enablement.clone(),
                });
            }
            if previous.active_state != service.active_state {
                changes.push(StateChange::ServiceRuntimeChanged {
                    service: key.clone(),
                    from: previous.active_state.clone(),
                    to: service.active_state.clone(),
                });
            }
        }
    }
    for (executable, process) in &after.facts.processes {
        let previous_count = before
            .facts
            .processes
            .get(executable)
            .map_or(0, |fact| fact.count);
        if previous_count != process.count {
            changes.push(StateChange::ProcessCountChanged {
                executable: executable.clone(),
                from: previous_count,
                to: process.count,
            });
        }
    }
    for (executable, process) in &before.facts.processes {
        if !after.facts.processes.contains_key(executable) && process.count > 0 {
            changes.push(StateChange::ProcessCountChanged {
                executable: executable.clone(),
                from: process.count,
                to: 0,
            });
        }
    }
    for (device, graphics) in &after.facts.graphics {
        let previous_driver = before
            .facts
            .graphics
            .get(device)
            .and_then(|fact| fact.driver.clone());
        if previous_driver != graphics.driver {
            changes.push(StateChange::GraphicsDriverChanged {
                device: device.clone(),
                from: previous_driver,
                to: graphics.driver.clone(),
            });
        }
    }
    for port in &after.facts.listening_ports {
        if !before.facts.listening_ports.contains(port) {
            changes.push(StateChange::PortOpened { port: port.clone() });
        }
    }
    for port in &before.facts.listening_ports {
        if !after.facts.listening_ports.contains(port) {
            changes.push(StateChange::PortClosed { port: port.clone() });
        }
    }
    for (name, interface) in &after.facts.network_interfaces {
        match before.facts.network_interfaces.get(name) {
            None => changes.push(StateChange::NetworkInterfaceAdded {
                name: name.clone(),
                interface: interface.clone(),
            }),
            Some(previous) if previous.operational_state != interface.operational_state => {
                changes.push(StateChange::NetworkInterfaceStateChanged {
                    name: name.clone(),
                    from: previous.operational_state.clone(),
                    to: interface.operational_state.clone(),
                });
            }
            _ => {}
        }
    }
    for (name, interface) in &before.facts.network_interfaces {
        if !after.facts.network_interfaces.contains_key(name) {
            changes.push(StateChange::NetworkInterfaceRemoved {
                name: name.clone(),
                interface: interface.clone(),
            });
        }
    }
    for (target, config) in &after.facts.config {
        if before.facts.config.get(target) != Some(config) {
            changes.push(StateChange::ConfigChanged {
                target: target.clone(),
            });
        }
    }
    for (runtime, fact) in &after.facts.runtimes {
        if let Some(previous) = before.facts.runtimes.get(runtime) {
            if previous.version != fact.version {
                changes.push(StateChange::RuntimeVersionChanged {
                    runtime: runtime.clone(),
                    from: previous.version.clone(),
                    to: fact.version.clone(),
                });
            }
        }
    }
    StateDiff {
        from: before.id.clone(),
        to: after.id.clone(),
        changes,
    }
}

pub fn diagnose(
    known_good: &SystemSnapshot,
    current: &SystemSnapshot,
    state_diff: &StateDiff,
) -> Vec<Diagnosis> {
    let docker = ServiceKey {
        scope: ServiceScope::System,
        name: "docker.service".into(),
    };
    let Some(service) = current.facts.services.get(&docker) else {
        return Vec::new();
    };
    let docker_installed = current.facts.packages.contains_key("docker");
    let socket_available = current
        .facts
        .runtimes
        .get("docker")
        .is_some_and(|runtime| runtime.available);
    let was_enabled = known_good
        .facts
        .services
        .get(&docker)
        .is_some_and(|fact| fact.enablement == Enablement::Enabled);
    let disabled_after_good = state_diff.changes.iter().any(|change| matches!(change, StateChange::ServiceEnablementChanged { service, from: Enablement::Enabled, to: Enablement::Disabled } if service == &docker));
    if docker_installed
        && was_enabled
        && disabled_after_good
        && service.enablement == Enablement::Disabled
        && (!socket_available || service.active_state != UnitState::Active)
    {
        return vec![Diagnosis {
            id: format!("docker-service-disabled@{}@{}", known_good.id, current.id),
            rule_id: "docker-service-disabled/v1".into(),
            confidence: Confidence::High,
            summary: "Docker is unavailable because docker.service was disabled after the last known-good snapshot.".into(),
            evidence: vec![
                Evidence { kind: "change".into(), detail: "docker.service changed from enabled to disabled".into() },
                Evidence { kind: "current-state".into(), detail: format!("docker.service is {:?}", service.active_state) },
                Evidence { kind: "dependency".into(), detail: "Docker CLI requires the Docker daemon and its socket".into() },
            ],
        }];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn snapshot(enablement: Enablement, active: UnitState, available: bool) -> SystemSnapshot {
        let key = ServiceKey {
            scope: ServiceScope::System,
            name: "docker.service".into(),
        };
        let facts = SnapshotFacts {
            packages: BTreeMap::from([(
                "docker".into(),
                PackageFact {
                    version: "28.0.0".into(),
                    install_reason: InstallReason::Explicit,
                    repository: Some("extra".into()),
                },
            )]),
            services: BTreeMap::from([(
                key,
                ServiceFact {
                    enablement,
                    active_state: active,
                    sub_state: None,
                },
            )]),
            runtimes: BTreeMap::from([(
                "docker".into(),
                RuntimeFact {
                    version: Some("28.0.0".into()),
                    available,
                },
            )]),
            ..Default::default()
        };
        SystemSnapshot::new("test-host".into(), Utc::now(), Utc::now(), vec![], facts)
    }

    #[test]
    fn detects_docker_disabled_after_known_good() {
        let good = snapshot(Enablement::Enabled, UnitState::Active, true);
        let broken = snapshot(Enablement::Disabled, UnitState::Inactive, false);
        let state_diff = diff(&good, &broken);
        let diagnoses = diagnose(&good, &broken, &state_diff);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].confidence, Confidence::High);
    }

    #[test]
    fn derives_docker_runtime_service_package_graph() {
        let snapshot = snapshot(Enablement::Enabled, UnitState::Active, true);
        let graph = dependency_graph(&snapshot);
        assert_eq!(graph.edges.len(), 2);
        assert!(graph.edges.iter().any(|edge| matches!(
            edge,
            DependencyEdge {
                from: DependencyNode::Runtime(runtime),
                relation: DependencyRelation::Requires,
                to: DependencyNode::Service(service),
            } if runtime == "docker" && service.name == "docker.service"
        )));
    }

    #[test]
    fn detects_graphics_driver_changes() {
        let mut before = snapshot(Enablement::Enabled, UnitState::Active, true);
        let mut after = snapshot(Enablement::Enabled, UnitState::Active, true);
        before.facts.graphics.insert(
            "gpu0".into(),
            GraphicsFact {
                driver: Some("nvidia".into()),
            },
        );
        after.facts.graphics.insert(
            "gpu0".into(),
            GraphicsFact {
                driver: Some("nouveau".into()),
            },
        );
        assert!(diff(&before, &after).changes.iter().any(|change| matches!(
            change,
            StateChange::GraphicsDriverChanged { device, from, to }
                if device == "gpu0" && from.as_deref() == Some("nvidia") && to.as_deref() == Some("nouveau")
        )));
    }
}
