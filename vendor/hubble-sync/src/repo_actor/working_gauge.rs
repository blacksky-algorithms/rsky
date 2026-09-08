use crate::metrics::REPO_ACTOR_WORKING;
use metrics::{Gauge, gauge};

pub(super) struct WorkingGauge(Gauge);

impl WorkingGauge {
    pub fn start(task: &'static str) -> Self {
        let g = gauge!(REPO_ACTOR_WORKING, "task" => task);
        g.increment(1.);
        Self(g)
    }
}

impl Drop for WorkingGauge {
    fn drop(&mut self) {
        self.0.decrement(1.);
    }
}
