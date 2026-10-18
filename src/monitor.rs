//! Resource observations collected independently of benchmark-round scheduling.
use crate::platform::{SystemMonitor, SystemState};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type Observation = (Instant, SystemState);

pub(crate) struct Monitor {
    latest: Arc<Mutex<Option<Observation>>>,
    stop: Arc<AtomicBool>,
    // Mutex supplies an unwind boundary for the opaque JoinHandle. No monitor
    // mutation exposes partially updated state: observations use their own Mutex,
    // shutdown uses an atomic, and this handle is consumed only under &mut self.
    worker: Mutex<Option<JoinHandle<()>>>,
    /// The sampler thread's effective mask, reported back through the startup
    /// channel after the requested pin (or inherited mask) took effect.
    /// `None` only if the affinity read itself failed.
    pub(crate) observer_effective_cpus: Option<Vec<usize>>,
}

impl Monitor {
    /// `observer_cpus` pins the sampler thread to CPUs inside the already
    /// verified process boundary but outside the coordinator/measurement
    /// mask (see [`CpuPlacement::select_observer`](crate::platform::CpuPlacement)).
    /// Selection outside the coordinator mask is not guaranteed dedicated
    /// isolation — external pools may already run there. `None` keeps the
    /// coordinator's pre-group inherited mask. The pin is applied before
    /// `SystemMonitor::for_benchmark()`, so the first CPU-delta interval and
    /// every refresh run on the observer CPUs. A refused mask reports through
    /// the startup channel, the worker is joined, and startup aborts — never
    /// a silent fallback onto the measured CPUs.
    pub(crate) fn start(observer_cpus: Option<Vec<usize>>) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let target = latest.clone();
        let done = stop.clone();
        let (ready, receive) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("resource-sampler".into())
            .spawn(move || {
                let _pin = match &observer_cpus {
                    Some(cpus) => match crate::platform::pin_current_thread(cpus) {
                        Ok(guard) => Some(guard),
                        Err(error) => {
                            let _ = ready.try_send(Err(format!(
                                "resource sampler affinity {cpus:?} refused: {error}"
                            )));
                            return;
                        }
                    },
                    None => None,
                };
                let effective = crate::platform::current_thread_affinity().ok();
                if std::env::var_os("ZENBENCH_GATE_TRACE").is_some() {
                    match &observer_cpus {
                        Some(cpus) => eprintln!(
                            "[zenbench gate] sampler affinity requested={cpus:?} effective={effective:?}"
                        ),
                        None => eprintln!(
                            "[zenbench gate] sampler affinity inherited effective={effective:?}"
                        ),
                    }
                }
                let monitor = SystemMonitor::for_benchmark();
                while !done.load(Ordering::Acquire) {
                    let state = monitor.snapshot();
                    *target.lock().unwrap() = Some((Instant::now(), state));
                    let _ = ready.try_send(Ok(effective.clone()));
                }
            })
            .expect("cannot start resource sampler");
        // The first CPU delta must span a real observation interval. This one
        // startup wait precedes warmup, never occurs between measured rounds.
        let observer_effective_cpus = match receive
            .recv()
            .expect("resource sampler failed before its initial observation")
        {
            Ok(effective) => effective,
            Err(error) => {
                let _ = worker.join();
                panic!("{error}");
            }
        };
        Self {
            latest,
            stop,
            worker: Mutex::new(Some(worker)),
            observer_effective_cpus,
        }
    }

    pub(crate) fn latest(&self) -> Option<(Duration, SystemState)> {
        let latest = self.latest.lock().ok()?;
        let (time, state) = latest.as_ref()?;
        let age = time.elapsed();
        (age <= Duration::from_secs(1)).then(|| (age, state.clone()))
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_reads_do_not_advance_or_wait_for_cpu_refresh() {
        let monitor = Monitor::start(None);
        let first = monitor.latest.lock().unwrap().as_ref().unwrap().0;
        for _ in 0..100 {
            assert!(monitor.latest().is_some());
        }
        // Directly control the cached observation instead of a timing threshold.
        monitor.stop.store(true, Ordering::Release);
        let mut monitor = monitor;
        monitor
            .worker
            .get_mut()
            .unwrap()
            .take()
            .unwrap()
            .join()
            .unwrap();
        monitor.latest.lock().unwrap().as_mut().unwrap().0 = first - Duration::from_secs(2);
        assert!(
            monitor.latest().is_none(),
            "stale observations must be unknown"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn pinned_sampler_reports_its_mask_and_survives_the_group_pin() {
        let original = crate::platform::current_thread_affinity().unwrap();
        if original.len() >= 2 {
            let observer = vec![original[1]];
            let monitor = Monitor::start(Some(observer.clone()));
            // The startup result carries the worker's live mask, exactly the
            // requested CPUs — read back after the pin, never assumed.
            assert_eq!(
                monitor.observer_effective_cpus.as_deref(),
                Some(observer.as_slice()),
                "sampler must run on the requested observer mask"
            );
            assert_eq!(
                crate::platform::current_thread_affinity().unwrap(),
                original,
                "starting the sampler must not change the coordinator mask"
            );
            // After the coordinator enters the narrowed group mask the
            // sampler's reported mask stays on the observer CPUs — it did
            // not inherit the group pin (it spawned before it).
            let _group = crate::platform::GroupAffinityGuard::enter(Some(&[original[0]]))
                .expect("test host must allow group pinning");
            assert_eq!(
                monitor.observer_effective_cpus.as_deref(),
                Some(observer.as_slice()),
                "sampler mask must not follow the group pin"
            );
        } else {
            // Single-CPU host: the engine selects the inherited fallback, so
            // `None` must leave the sampler on the coordinator's mask.
            let monitor = Monitor::start(None);
            assert_eq!(
                monitor.observer_effective_cpus.as_deref(),
                Some(original.as_slice()),
                "unpinned sampler must inherit the coordinator's mask"
            );
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn unpinned_sampler_reports_the_inherited_pregroup_mask() {
        let original = crate::platform::current_thread_affinity().unwrap();
        let monitor = Monitor::start(None);
        assert_eq!(
            monitor.observer_effective_cpus.as_deref(),
            Some(original.as_slice()),
            "fallback must be the coordinator's pre-group mask, not a narrowed one"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn refused_observer_mask_aborts_and_joins_the_worker() {
        let before = crate::platform::current_thread_affinity().unwrap();
        let result = std::panic::catch_unwind(|| Monitor::start(Some(vec![usize::MAX])));
        let panic = result.err().expect("an impossible CPU must be refused");
        let message = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        assert!(
            message.contains("refused"),
            "panic must carry the pin refusal: {message}"
        );
        // Joined before propagating: a second start succeeds and observes.
        let monitor = Monitor::start(None);
        assert!(monitor.latest().is_some());
        assert_eq!(
            crate::platform::current_thread_affinity().unwrap(),
            before,
            "failed startup must not change the coordinator mask"
        );
    }
}
