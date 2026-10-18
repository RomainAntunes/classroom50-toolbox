//! How far a sync is, as one fraction from 0 to 1 that never steps back. A sync runs in
//! phases (some at once); each gets a share of the bar sized by how long it took the last
//! time the same kind of sync ran, and fills its share as its own steps complete. Until a
//! phase knows how many steps it has, the last run's count stands in.

use std::collections::BTreeMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Pages of the repo list, plus the teams query.
    Listing,
    /// `.classroom50.yaml` batches for repos pushed since the last sync, and group members.
    Details,
    /// Each classroom's `assignments.json`.
    Manifests,
    /// Status batches of the assignment repos.
    Statuses,
    /// Folders scanned for clones.
    Scan,
    /// `git status` and the comparison with GitHub of each clone.
    Clones,
}

impl Phase {
    /// Seconds a phase is assumed to take before it has ever been measured.
    fn default_seconds(self) -> f32 {
        match self {
            Phase::Listing => 2.0,
            Phase::Details => 2.0,
            Phase::Manifests => 0.5,
            Phase::Statuses => 3.0,
            Phase::Scan => 0.5,
            Phase::Clones => 0.5,
        }
    }
}

/// What a phase cost in the last run of a kind of sync.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PhaseCost {
    #[cfg_attr(test, specta(type = specta_typescript::Number))]
    pub seconds: f32,
    pub steps: usize,
}

/// The phases of one kind of sync, by what they cost last time.
pub type Costs = BTreeMap<Phase, PhaseCost>;

#[derive(Debug, Default)]
struct PhaseState {
    done: usize,
    /// `None` until the phase knows its step count.
    total: Option<usize>,
    started: Option<Instant>,
    seconds: Option<f32>,
    skipped: bool,
}

#[derive(Debug)]
pub struct Tracker {
    /// Each phase's share of the bar; they add up to 1.
    shares: BTreeMap<Phase, f32>,
    estimates: BTreeMap<Phase, usize>,
    phases: BTreeMap<Phase, PhaseState>,
    reported: f32,
}

impl Tracker {
    /// `phases` are those this kind of sync runs; `last` what they cost the last time.
    pub fn new(phases: &[Phase], last: &Costs) -> Self {
        let seconds = |p: &Phase| {
            last.get(p)
                .map(|c| c.seconds)
                .filter(|s| s.is_finite() && *s > 0.0)
                .unwrap_or_else(|| p.default_seconds())
                // A phase that was instant last time still gets a sliver.
                .max(0.05)
        };
        let sum: f32 = phases.iter().map(seconds).sum();
        Self {
            shares: phases.iter().map(|p| (*p, seconds(p) / sum)).collect(),
            estimates: phases
                .iter()
                .filter_map(|p| Some((*p, last.get(p)?.steps)))
                .collect(),
            phases: phases.iter().map(|p| (*p, PhaseState::default())).collect(),
            reported: 0.0,
        }
    }

    fn phase(&mut self, phase: Phase) -> &mut PhaseState {
        let state = self.phases.entry(phase).or_default();
        state.started.get_or_insert_with(Instant::now);
        state
    }

    /// The phase now knows how many steps it has.
    pub fn set_total(&mut self, phase: Phase, total: usize) {
        self.phase(phase).total = Some(total);
    }

    pub fn advance(&mut self, phase: Phase) {
        self.phase(phase).done += 1;
    }

    pub fn finish(&mut self, phase: Phase) {
        let state = self.phase(phase);
        if state.seconds.is_none() {
            state.seconds = state.started.map(|s| s.elapsed().as_secs_f32());
        }
    }

    /// The phase won't run, or won't finish, this time (no GitHub access): its share
    /// counts as done, and its cost from the last run is kept. A finished phase stays so.
    pub fn skip(&mut self, phase: Phase) {
        let state = self.phases.entry(phase).or_default();
        state.skipped = state.seconds.is_none();
    }

    fn ratio(&self, phase: Phase, state: &PhaseState) -> f32 {
        if state.skipped || state.seconds.is_some() {
            return 1.0;
        }
        let total = state
            .total
            .or_else(|| self.estimates.get(&phase).copied())
            .unwrap_or(0);
        if total == 0 {
            return 0.0;
        }
        // Full only once the phase says it's finished.
        (state.done as f32 / total as f32).min(0.95)
    }

    /// The fraction to show: the weighted sum of the phases, never lower than before.
    pub fn fraction(&mut self) -> f32 {
        let sum: f32 = self
            .phases
            .iter()
            .map(|(phase, state)| {
                self.shares.get(phase).copied().unwrap_or(0.0) * self.ratio(*phase, state)
            })
            .sum();
        self.reported = self.reported.max(sum.min(1.0));
        self.reported
    }

    /// What each finished phase cost, to size the bar next time. Skipped phases and those
    /// that never ran keep their last cost.
    pub fn costs(&self, last: &Costs) -> Costs {
        let mut costs = last.clone();
        for (phase, state) in &self.phases {
            if let (false, Some(seconds)) = (state.skipped, state.seconds) {
                costs.insert(
                    *phase,
                    PhaseCost {
                        seconds,
                        steps: state.total.unwrap_or(state.done),
                    },
                );
            }
        }
        costs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(seconds: f32, steps: usize) -> PhaseCost {
        PhaseCost { seconds, steps }
    }

    #[test]
    fn heavier_phases_get_a_bigger_share() {
        let last = Costs::from([(Phase::Scan, cost(1.0, 4)), (Phase::Statuses, cost(3.0, 2))]);
        let mut tracker = Tracker::new(&[Phase::Scan, Phase::Statuses], &last);
        tracker.set_total(Phase::Scan, 4);
        for _ in 0..4 {
            tracker.advance(Phase::Scan);
        }
        tracker.finish(Phase::Scan);
        assert!((tracker.fraction() - 0.25).abs() < 1e-4);
        tracker.finish(Phase::Statuses);
        assert!((tracker.fraction() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn never_steps_back_when_more_work_is_found() {
        let last = Costs::from([(Phase::Details, cost(1.0, 1))]);
        let mut tracker = Tracker::new(&[Phase::Details], &last);
        tracker.advance(Phase::Details);
        let before = tracker.fraction();
        tracker.set_total(Phase::Details, 10);
        assert!(tracker.fraction() >= before);
    }

    #[test]
    fn last_runs_step_count_stands_in_until_known() {
        let last = Costs::from([(Phase::Listing, cost(1.0, 4))]);
        let mut tracker = Tracker::new(&[Phase::Listing], &last);
        tracker.advance(Phase::Listing);
        assert!((tracker.fraction() - 0.25).abs() < 1e-4);
    }

    #[test]
    fn skipped_phases_count_as_done_and_keep_their_cost() {
        let last = Costs::from([(Phase::Statuses, cost(5.0, 3))]);
        let mut tracker = Tracker::new(&[Phase::Clones, Phase::Statuses], &last);
        tracker.skip(Phase::Statuses);
        assert!(tracker.fraction() > 0.8);
        tracker.finish(Phase::Clones);
        let costs = tracker.costs(&last);
        assert_eq!(costs[&Phase::Statuses], cost(5.0, 3));
        assert!(costs.contains_key(&Phase::Clones));
    }

    #[test]
    fn unfinished_phase_stops_short_of_full() {
        let mut tracker = Tracker::new(&[Phase::Scan], &Costs::new());
        tracker.set_total(Phase::Scan, 1);
        tracker.advance(Phase::Scan);
        assert!(tracker.fraction() < 1.0);
    }
}
