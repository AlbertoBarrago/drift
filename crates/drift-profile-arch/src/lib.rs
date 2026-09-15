use drift_domain::{Diagnosis, StateDiff, SystemSnapshot};
use drift_engine::DiagnosisProfile;

pub struct ArchProfile;

impl DiagnosisProfile for ArchProfile {
    fn id(&self) -> &'static str {
        "arch"
    }

    fn diagnose(
        &self,
        _known_good: &SystemSnapshot,
        _current: &SystemSnapshot,
        _state_diff: &StateDiff,
    ) -> Vec<Diagnosis> {
        Vec::new()
    }
}
