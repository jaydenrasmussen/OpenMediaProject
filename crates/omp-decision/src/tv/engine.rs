//! Runs the TV specs in priority groups, like the movie engine.

use super::{
    model::{EpisodeSubject, TvDecisionData, TvDecisionInfo},
    specs::{TvSpecification, default_tv_specs},
};
use crate::{
    engine::Decision,
    reason::{Rejection, Verdict},
    specs::Priority,
};

pub struct TvDecisionEngine {
    specs: Vec<Box<dyn TvSpecification>>,
}

impl Default for TvDecisionEngine {
    fn default() -> Self {
        Self::new(default_tv_specs())
    }
}

impl TvDecisionEngine {
    pub fn new(specs: Vec<Box<dyn TvSpecification>>) -> Self {
        Self { specs }
    }

    /// Every spec in a group runs so the person sees all reasons; the first
    /// group with a rejection ends the run.
    pub fn decide(
        &self,
        subject: &EpisodeSubject<'_>,
        info: &TvDecisionInfo,
        env: &dyn TvDecisionData,
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
