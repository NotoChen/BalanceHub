//! Coalesce concurrent running-state checks without using the inventory TTL as
//! file authority. Writes/launches always request a fresh observation.
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

struct Observation {
    completed: Instant,
    result: Result<Vec<String>, String>,
}

static OBSERVATION: Mutex<Option<Observation>> = Mutex::new(None);

pub(super) fn names(fresh: bool) -> Result<Vec<String>, String> {
    let requested = Instant::now();
    let mut state = OBSERVATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(observation) = state.as_ref() {
        if observation.completed >= requested
            || (!fresh && observation.completed.elapsed() < Duration::from_millis(250))
        {
            return observation.result.clone();
        }
    }
    let result = super::probe::names(true);
    *state = Some(Observation {
        completed: Instant::now(),
        result: result.clone(),
    });
    result
}
