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
    pub(crate) fn start() -> Self {
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let target = latest.clone();
        let done = stop.clone();
        let (ready, receive) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("resource-sampler".into())
            .spawn(move || {
                let monitor = SystemMonitor::for_benchmark();
                while !done.load(Ordering::Acquire) {
                    let state = monitor.snapshot();
                    *target.lock().unwrap() = Some((Instant::now(), state));
                    let _ = ready.try_send(());
                }
            })
            .expect("cannot start resource sampler");
        // The first CPU delta must span a real observation interval. This one
        // startup wait precedes warmup, never occurs between measured rounds.
        receive
            .recv()
            .expect("resource sampler failed before its initial observation");
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
        let monitor = Monitor::start();
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
