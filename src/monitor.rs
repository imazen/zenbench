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
}

impl Monitor {
    /// `observer_cpus` pins the sampler thread to CPUs inside the already
    /// verified process boundary but outside the coordinator/measurement
    /// mask (see [`CpuPlacement::observer_cpus`](crate::platform::CpuPlacement)).
    /// That is selection outside the coordinator mask, not guaranteed
    /// dedicated isolation — external pools may already occupy those CPUs.
    /// `None` keeps ordinary inherited affinity. A requested mask failure
    /// propagates through the startup channel and aborts startup — never a
    /// silent fallback onto the measured CPUs.
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
                        Ok(guard) => {
                            if std::env::var_os("ZENBENCH_GATE_TRACE").is_some() {
                                eprintln!(
                                    "[zenbench gate] sampler affinity requested={cpus:?} effective={:?}",
                                    crate::platform::current_thread_affinity()
                                );
                            }
                            Some(guard)
                        }
                        Err(error) => {
                            let _ = ready.try_send(Err(format!(
                                "resource sampler affinity {cpus:?} refused: {error}"
                            )));
                            return;
                        }
                    },
                    None => None,
                };
                let monitor = SystemMonitor::for_benchmark();
                while !done.load(Ordering::Acquire) {
                    let state = monitor.snapshot();
                    *target.lock().unwrap() = Some((Instant::now(), state));
                    let _ = ready.try_send(Ok(()));
                }
            })
            .expect("cannot start resource sampler");
        // The first CPU delta must span a real observation interval. This one
        // startup wait precedes warmup, never occurs between measured rounds.
        receive
            .recv()
            .expect("resource sampler failed before its initial observation")
            .unwrap_or_else(|error| panic!("{error}"));
        Self {
            latest,
            stop,
            worker: Mutex::new(Some(worker)),
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
}
