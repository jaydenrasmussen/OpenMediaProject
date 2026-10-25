//! Runs the specs. Ported from Radarr `DownloadDecisionMaker.GetDecisionForReport`.

use crate::{
    model::{DecisionData, DecisionInfo, Subject},
    reason::{Rejection, RejectionKind, Verdict},
    specs::{Priority, Specification, default_specs},
};

/// The outcome for one release.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decision {
    pub rejections: Vec<Rejection>,
}

impl Decision {
    pub fn approved(&self) -> bool {
        self.rejections.is_empty()
    }

    /// Rejected only for reasons that may pass later (a delay, a blocked
    /// indexer). Such releases go to the pending queue.
    pub fn temporarily_rejected(&self) -> bool {
        !self.rejections.is_empty()
            && self
                .rejections
                .iter()
                .all(|r| r.kind == RejectionKind::Temporary)
    }

    pub fn rejected(&self) -> bool {
        self.rejections
            .iter()
            .any(|r| r.kind == RejectionKind::Permanent)
    }
}

pub struct DecisionEngine {
    specs: Vec<Box<dyn Specification>>,
}

impl Default for DecisionEngine {
    fn default() -> Self {
        Self::new(default_specs())
    }
}

impl DecisionEngine {
    pub fn new(specs: Vec<Box<dyn Specification>>) -> Self {
        Self { specs }
    }

    /// Specs run in priority groups, cheapest first. Every spec in a group
    /// runs, so the user sees all reasons; the first group that rejects ends
    /// the run, so later groups (disk access) are skipped.
    pub fn decide(
        &self,
        subject: &Subject<'_>,
        info: &DecisionInfo,
        env: &dyn DecisionData,
    ) -> Decision {
        let mut priorities: Vec<Priority> = self.specs.iter().map(|s| s.priority()).collect();
        priorities.sort_unstable();
        priorities.dedup();

        for priority in priorities {
            let rejections: Vec<Rejection> = self
                .specs
                .iter()
                .filter(|s| s.priority() == priority)
                .filter_map(|spec| match spec.evaluate(subject, info, env) {
                    Verdict::Accept => None,
                    Verdict::Reject { reason, message } => Some(Rejection {
                        reason,
                        message,
                        kind: spec.kind(),
                    }),
                })
                .collect();
            if !rejections.is_empty() {
                return Decision { rejections };
            }
        }
        Decision::default()
    }
}
