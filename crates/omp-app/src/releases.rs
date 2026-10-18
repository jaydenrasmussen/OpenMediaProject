//! Interactive search results, held for a while so the release a person picks
//! can be grabbed without searching again.

use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use omp_services::Evaluation;

const KEEP: Duration = Duration::from_secs(30 * 60);
const MAX_SEARCHES: usize = 8;

#[derive(Default)]
pub struct SearchCache {
    next_id: u64,
    entries: VecDeque<(u64, Instant, Arc<Evaluation>)>,
}

impl SearchCache {
    pub fn insert(&mut self, evaluation: Evaluation) -> (u64, Arc<Evaluation>) {
        self.entries.retain(|(_, at, _)| at.elapsed() < KEEP);
        while self.entries.len() >= MAX_SEARCHES {
            self.entries.pop_front();
        }
        self.next_id += 1;
        let shared = Arc::new(evaluation);
        self.entries
            .push_back((self.next_id, Instant::now(), shared.clone()));
        (self.next_id, shared)
    }

    pub fn get(&self, id: u64) -> Option<Arc<Evaluation>> {
        self.entries
            .iter()
            .find(|(i, at, _)| *i == id && at.elapsed() < KEEP)
            .map(|(_, _, e)| e.clone())
    }
}
