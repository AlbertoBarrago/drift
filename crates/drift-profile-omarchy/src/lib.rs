use drift_collect::{SnapshotExtension, SystemProbe};
use drift_domain::{
    CollectorOutcome, CollectorReport, Confidence, Diagnosis, Evidence, SnapshotFacts, StateChange,
    StateDiff, SystemSnapshot,
};
use drift_engine::DiagnosisProfile;

pub struct OmarchyProfile;

impl SnapshotExtension for OmarchyProfile {
    fn collect(&self, probe: &dyn SystemProbe, facts: &mut SnapshotFacts) -> CollectorReport {
        let Some(home) = probe.home_dir() else {
            return CollectorReport {
                name: "omarchy".into(),
                version: "1".into(),
                outcome: CollectorOutcome::Unavailable,
                detail: Some("home directory unavailable".into()),
            };
        };
        for path in [
            home.join(".config/waybar/config"),
            home.join(".config/waybar/config.jsonc"),
        ] {
            if let Ok(contents) = probe.read_file(&path) {
                facts
                    .waybar_referenced_interfaces
                    .extend(extract_interfaces(&contents));
            }
        }
        CollectorReport {
            name: "omarchy".into(),
            version: "1".into(),
            outcome: CollectorOutcome::Complete,
            detail: None,
        }
    }
}

impl DiagnosisProfile for OmarchyProfile {
    fn id(&self) -> &'static str {
        "omarchy"
    }

    fn diagnose(
        &self,
        known_good: &SystemSnapshot,
        current: &SystemSnapshot,
        state_diff: &StateDiff,
    ) -> Vec<Diagnosis> {
        current.facts.waybar_referenced_interfaces.iter().filter(|interface| !current.facts.network_interfaces.contains_key(*interface)).map(|interface| {
            let interface_removed = known_good.facts.network_interfaces.contains_key(interface);
            let waybar_config_changed = state_diff.changes.iter().any(|change| matches!(change, StateChange::ConfigChanged { target } if target.starts_with("waybar/")));
            Diagnosis {
                id: format!("waybar-interface-missing@{}@{}@{}", known_good.id, current.id, interface),
                rule_id: "omarchy-waybar-interface-missing/v1".into(),
                confidence: if interface_removed || waybar_config_changed { Confidence::High } else { Confidence::Medium },
                summary: format!("Waybar references missing network interface {interface}."),
                evidence: vec![
                    Evidence { kind: "configuration".into(), detail: format!("Waybar references interface {interface}") },
                    Evidence { kind: "current-state".into(), detail: format!("Network interface {interface} is absent") },
                ],
            }
        }).collect()
    }

    fn graph_edges(&self, snapshot: &SystemSnapshot) -> Vec<drift_domain::DependencyEdge> {
        snapshot
            .facts
            .waybar_referenced_interfaces
            .iter()
            .map(|interface| drift_domain::DependencyEdge {
                from: drift_domain::DependencyNode::Config("waybar".into()),
                relation: drift_domain::DependencyRelation::References,
                to: drift_domain::DependencyNode::NetworkInterface(interface.clone()),
            })
            .collect()
    }
}

fn extract_interfaces(contents: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(contents);
    let mut interfaces = Vec::new();
    let mut remaining = text.as_ref();
    while let Some(index) = remaining.find("\"interface\"") {
        remaining = &remaining[index + "\"interface\"".len()..];
        let Some(value_start) = remaining.find(':').and_then(|index| {
            remaining[index + 1..]
                .find('"')
                .map(|offset| index + offset + 2)
        }) else {
            continue;
        };
        remaining = &remaining[value_start..];
        let Some(value_end) = remaining.find('"') else {
            break;
        };
        let value = &remaining[..value_end];
        if !value.is_empty() && !value.contains(char::is_whitespace) {
            interfaces.push(value.into());
        }
        remaining = &remaining[value_end + 1..];
    }
    interfaces
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use chrono::Utc;
    use drift_domain::{Confidence, NetworkInterfaceFact, SnapshotFacts, SystemSnapshot};
    use drift_engine::diff;
    use serde::Deserialize;

    use super::{DiagnosisProfile, OmarchyProfile, extract_interfaces};

    #[test]
    fn extracts_waybar_interfaces_without_storing_config_content() {
        assert_eq!(
            extract_interfaces(b"// comment\n{ \"interface\": \"wlan0\" }"),
            vec!["wlan0"]
        );
    }

    #[test]
    fn diagnoses_a_missing_waybar_interface_from_fixture() {
        #[derive(Deserialize)]
        struct Fixture {
            network_interfaces: BTreeMap<String, String>,
            waybar_referenced_interfaces: BTreeSet<String>,
        }

        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../fixtures/broken_waybar_interface.json"
        ))
        .expect("valid Waybar fixture");
        let known_good = SystemSnapshot::new(
            "fixture-host".into(),
            Utc::now(),
            Utc::now(),
            vec![],
            SnapshotFacts {
                network_interfaces: BTreeMap::from([(
                    "wlan0".into(),
                    NetworkInterfaceFact {
                        operational_state: "UP".into(),
                    },
                )]),
                waybar_referenced_interfaces: BTreeSet::from(["wlan0".into()]),
                ..Default::default()
            },
        );
        let current = SystemSnapshot::new(
            "fixture-host".into(),
            Utc::now(),
            Utc::now(),
            vec![],
            SnapshotFacts {
                network_interfaces: fixture
                    .network_interfaces
                    .into_iter()
                    .map(|(name, operational_state)| {
                        (name, NetworkInterfaceFact { operational_state })
                    })
                    .collect(),
                waybar_referenced_interfaces: fixture.waybar_referenced_interfaces,
                ..Default::default()
            },
        );
        let diagnoses =
            OmarchyProfile.diagnose(&known_good, &current, &diff(&known_good, &current));
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].confidence, Confidence::High);
    }
}
