//! Private macOS process rates independent of sysinfo's percentage denominator.
use std::{collections::BTreeMap, time::Instant};
use sysinfo::Pid;

pub(super) struct CpuCounters {
    at: Instant,
    previous: BTreeMap<Pid, (u64, u64)>,
}
impl CpuCounters {
    pub(super) fn new(
        at: Instant,
        observations: impl IntoIterator<Item = (Pid, u64, u64)>,
    ) -> Self {
        Self {
            at,
            previous: observations
                .into_iter()
                .map(|(pid, start, cpu)| (pid, (start, cpu)))
                .collect(),
        }
    }
    pub(super) fn refresh(
        &mut self,
        at: Instant,
        observations: impl IntoIterator<Item = (Pid, u64, u64)>,
    ) -> BTreeMap<Pid, f64> {
        let current: BTreeMap<_, _> = observations
            .into_iter()
            .map(|(pid, start, cpu)| (pid, (start, cpu)))
            .collect();
        let elapsed = at.saturating_duration_since(self.at).as_secs_f64();
        let rates = current
            .iter()
            .filter_map(|(&pid, &(start, cpu))| {
                let &(previous_start, previous_cpu) = self.previous.get(&pid)?;
                if start != previous_start || elapsed <= 0.0 {
                    return None;
                }
                let delta = cpu.checked_sub(previous_cpu)?;
                Some((pid, delta as f64 / elapsed / 10.0))
            })
            .collect();
        self.at = at;
        self.previous = current;
        rates
    }
}

pub(super) fn conservative_percent(raw: f64, rate: Option<f64>, owned: bool) -> f64 {
    // No matching history must never authorize subtraction of an owner's CPU.
    // Unknown foreign work retains its raw signal as well as global CPU load.
    rate.unwrap_or(if owned { 0.0 } else { raw })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn mac_underreported_owner_percentage_uses_same_interval_cumulative_cpu() {
        let pid = Pid::from_u32(35262);
        let at = Instant::now();
        let mut counters = CpuCounters::new(at, [(pid, 12, 1490)]);
        let rates = counters.refresh(at + Duration::from_micros(202086), [(pid, 12, 2353)]);
        let pct = rates[&pid];
        assert!((pct - 427.0459111467395).abs() < 1e-10);
        let corrected = conservative_percent(10.355022430419922, Some(pct), true);
        let (foreign, heavy) =
            super::super::foreign_activity(0.4857978185017904, 12, [(pid, corrected)], &[pid], &[]);
        assert!(foreign < 0.20);
        assert_eq!(heavy, 0);
        let (historical, _) = super::super::foreign_activity(
            0.4857978185017904,
            12,
            [(pid, 10.355022430419922)],
            &[pid],
            &[],
        );
        assert!((historical - 0.4771686331431071).abs() < 1e-12);
    }

    #[test]
    fn real_foreign_busy_work_stays_visible_and_heavy() {
        let own = Pid::from_u32(1);
        let rival = Pid::from_u32(2);
        let at = Instant::now();
        let mut counters = CpuCounters::new(at, [(own, 10, 1000), (rival, 20, 1000)]);
        let rates = counters.refresh(
            at + Duration::from_millis(200),
            [(own, 10, 1800), (rival, 20, 1800)],
        );
        let (load, heavy) = super::super::foreign_activity(
            0.75,
            12,
            [(own, rates[&own]), (rival, rates[&rival])],
            &[own],
            &[],
        );
        assert!((load - (0.75 - 4.0 / 12.0)).abs() < 1e-12);
        assert!(load > 0.20);
        assert_eq!(heavy, 1);
    }

    #[test]
    fn new_reused_reset_missing_and_stale_processes_cannot_hide_work() {
        let pid = Pid::from_u32(1);
        let at = Instant::now();
        let mut c = CpuCounters::new(at, [(pid, 10, 500)]);
        assert!(
            !c.refresh(at + Duration::from_millis(200), [(pid, 11, 900)])
                .contains_key(&pid)
        );
        assert!(
            !c.refresh(at + Duration::from_millis(400), [(pid, 11, 1)])
                .contains_key(&pid)
        );
        assert_eq!(
            c.refresh(at + Duration::from_millis(600), [(pid, 11, 1)])[&pid],
            0.0
        );
        assert!(c.refresh(at + Duration::from_millis(800), []).is_empty());
        assert!(
            !c.refresh(at + Duration::from_millis(1000), [(pid, 11, 900)])
                .contains_key(&pid)
        );
        assert_eq!(conservative_percent(800.0, None, true), 0.0);
        assert_eq!(conservative_percent(100.0, None, false), 100.0);
    }
}
