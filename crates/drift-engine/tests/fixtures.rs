use std::collections::{BTreeMap, BTreeSet};

use chrono::Utc;
use drift_domain::*;
use drift_engine::{diagnose, diff};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    kernel_release: String,
    packages: BTreeMap<String, String>,
    services: Vec<FixtureService>,
    runtimes: BTreeMap<String, FixtureRuntime>,
    graphics: BTreeMap<String, String>,
    network_interfaces: BTreeMap<String, String>,
    waybar_referenced_interfaces: BTreeSet<String>,
}

#[derive(Deserialize)]
struct FixtureService {
    scope: String,
    name: String,
    enablement: String,
    active_state: String,
}

#[derive(Deserialize)]
struct FixtureRuntime {
    version: Option<String>,
    available: bool,
}

fn snapshot(json: &str) -> SystemSnapshot {
    let fixture: Fixture = serde_json::from_str(json).expect("valid fixture JSON");
    let services = fixture
        .services
        .into_iter()
        .map(|service| {
            let scope = match service.scope.as_str() {
                "system" => ServiceScope::System,
                "user" => ServiceScope::User,
                _ => panic!("unknown fixture scope"),
            };
            let enablement = match service.enablement.as_str() {
                "enabled" => Enablement::Enabled,
                "disabled" => Enablement::Disabled,
                _ => Enablement::Unknown,
            };
            let active_state = match service.active_state.as_str() {
                "active" => UnitState::Active,
                "inactive" => UnitState::Inactive,
                _ => UnitState::Unknown,
            };
            (
                ServiceKey {
                    scope,
                    name: service.name,
                },
                ServiceFact {
                    enablement,
                    active_state,
                    sub_state: None,
                },
            )
        })
        .collect();
    SystemSnapshot::new(
        "fixture-host".into(),
        Utc::now(),
        Utc::now(),
        vec![],
        SnapshotFacts {
            kernel_release: Some(fixture.kernel_release),
            packages: fixture
                .packages
                .into_iter()
                .map(|(name, version)| {
                    (
                        name,
                        PackageFact {
                            version,
                            install_reason: InstallReason::Explicit,
                            repository: Some("fixture".into()),
                        },
                    )
                })
                .collect(),
            services,
            runtimes: fixture
                .runtimes
                .into_iter()
                .map(|(name, runtime)| {
                    (
                        name,
                        RuntimeFact {
                            version: runtime.version,
                            available: runtime.available,
                        },
                    )
                })
                .collect(),
            graphics: fixture
                .graphics
                .into_iter()
                .map(|(device, driver)| {
                    (
                        device,
                        GraphicsFact {
                            driver: Some(driver),
                        },
                    )
                })
                .collect(),
            network_interfaces: fixture
                .network_interfaces
                .into_iter()
                .map(|(name, operational_state)| (name, NetworkInterfaceFact { operational_state }))
                .collect(),
            waybar_referenced_interfaces: fixture.waybar_referenced_interfaces,
            ..Default::default()
        },
    )
}

fn healthy() -> SystemSnapshot {
    snapshot(include_str!("../../../fixtures/healthy_arch.json"))
}

#[test]
fn docker_disabled_fixture_produces_a_high_confidence_diagnosis() {
    let before = healthy();
    let after = snapshot(include_str!("../../../fixtures/docker_disabled.json"));
    let diagnoses = diagnose(&before, &after, &diff(&before, &after));
    assert_eq!(diagnoses.len(), 1);
    assert_eq!(diagnoses[0].confidence, Confidence::High);
}

#[test]
fn node_regression_fixture_produces_a_runtime_version_change() {
    let before = healthy();
    let after = snapshot(include_str!(
        "../../../fixtures/node_version_regression.json"
    ));
    assert!(diff(&before, &after).changes.iter().any(|change| matches!(
        change,
        StateChange::RuntimeVersionChanged { runtime, from, to }
            if runtime == "node" && from.as_deref() == Some("v22.15.0") && to.as_deref() == Some("v20.19.0")
    )));
}

#[test]
fn kernel_driver_regression_fixture_produces_kernel_and_driver_changes() {
    let before = healthy();
    let after = snapshot(include_str!(
        "../../../fixtures/kernel_driver_regression.json"
    ));
    let changes = diff(&before, &after).changes;
    assert!(
        changes
            .iter()
            .any(|change| matches!(change, StateChange::KernelVersionChanged { .. }))
    );
    assert!(changes.iter().any(|change| matches!(change, StateChange::GraphicsDriverChanged { device, from, to } if device == "gpu0" && from.as_deref() == Some("nvidia") && to.as_deref() == Some("nouveau"))));
}
