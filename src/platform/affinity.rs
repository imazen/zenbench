//! Scoped thread affinity. Linux/Windows use the safe `affinity` API; macOS
//! uses a small Mach-policy backend with a scoped unsafe-code exception.

use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::OnceLock;

/// An affinity request could not be parsed or applied.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AffinityError {
    /// At least one CPU must be requested.
    EmptyCoreSet,
    /// A CPU ID is outside the backend's representable range.
    InvalidCore(usize),
    /// Invalid `ZENBENCH_PIN` syntax (CPU IDs and inclusive ranges, comma separated).
    InvalidSpecification(String),
    /// Thread affinity is unavailable on this target.
    Unsupported,
    /// The operating system rejected the request.
    Os(String),
}

impl fmt::Display for AffinityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCoreSet => write!(f, "affinity needs at least one CPU"),
            Self::InvalidCore(id) => write!(f, "CPU ID {id} is outside the affinity mask"),
            Self::InvalidSpecification(spec) => write!(f, "invalid ZENBENCH_PIN: {spec:?}"),
            Self::Unsupported => write!(f, "thread affinity is unsupported on this target"),
            Self::Os(message) => write!(f, "thread affinity: {message}"),
        }
    }
}

impl std::error::Error for AffinityError {}

/// Restores the calling thread's previous affinity when dropped.
///
/// This guard cannot move between threads. Nest guards in stack order. Restoration
/// failures (for example, CPUs taken offline during the scope) are reported to stderr.
///
/// ```compile_fail
/// let guard = zenbench::platform::pin_current_thread(&[0]).unwrap();
/// std::thread::spawn(move || drop(guard)); // Must restore on the original thread.
/// ```
#[must_use = "dropping the guard restores the previous thread affinity"]
#[derive(Debug)]
pub struct AffinityGuard {
    previous: Option<backend::Saved>,
    _thread_bound: PhantomData<Rc<()>>,
}

/// Restrict the calling thread to the given logical CPU IDs for a scope.
///
/// Linux uses `sched_getaffinity` / `sched_setaffinity` and verifies the complete
/// requested mask, rejecting silently filtered CPUs. Windows uses the calling
/// thread's current processor group (IDs below `usize::BITS`). Existing thread
/// pools are unaffected. Empty CPU sets and unsupported targets return errors.
///
/// macOS maps the CPU set to an opaque `THREAD_AFFINITY_POLICY` tag: this is a
/// cache-sharing hint, **not a CPU binding**. The scheduler may ignore it. Equal
/// sets use equal tags within a process. Unsupported Mach policies return errors.
///
/// Keep the returned guard alive while benchmarking; dropping it restores the mask,
/// including during panic unwinding.
pub fn pin_current_thread(cores: &[usize]) -> Result<AffinityGuard, AffinityError> {
    if cores.is_empty() {
        return Err(AffinityError::EmptyCoreSet);
    }
    let previous = backend::pin(cores)?;
    Ok(AffinityGuard {
        previous: Some(previous),
        _thread_bound: PhantomData,
    })
}

/// Query the calling thread's permitted logical CPU IDs.
///
/// Windows IDs are relative to the current processor group. Unsupported targets
/// and macOS (which has tags rather than CPU masks) return
/// [`AffinityError::Unsupported`].
pub fn current_thread_affinity() -> Result<Vec<usize>, AffinityError> {
    backend::current()
}

impl Drop for AffinityGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            if let Err(error) = backend::restore(previous) {
                eprintln!("[zenbench] could not restore thread affinity: {error}");
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod backend {
    use super::AffinityError;

    pub(super) type Saved = Vec<usize>;

    pub(super) fn current() -> Result<Vec<usize>, AffinityError> {
        let cores =
            affinity::get_thread_affinity().map_err(|e| AffinityError::Os(e.to_string()))?;
        if cores.is_empty() {
            return Err(AffinityError::Os(
                "OS returned an empty affinity mask".into(),
            ));
        }
        Ok(cores)
    }

    pub(super) fn pin(cores: &[usize]) -> Result<Saved, AffinityError> {
        #[cfg(target_os = "linux")]
        let limit = libc::CPU_SETSIZE as usize;
        #[cfg(target_os = "windows")]
        let limit = usize::BITS as usize;
        if let Some(&id) = cores.iter().find(|&&id| id >= limit) {
            return Err(AffinityError::InvalidCore(id));
        }
        let mut requested = cores.to_vec();
        requested.sort_unstable();
        requested.dedup();
        let previous = current()?;
        let mut guard = super::AffinityGuard {
            previous: Some(previous),
            _thread_bound: std::marker::PhantomData,
        };
        affinity::set_thread_affinity(&requested).map_err(|e| AffinityError::Os(e.to_string()))?;
        if current()? != requested {
            return Err(AffinityError::Os(
                "OS did not apply the entire requested CPU set".into(),
            ));
        }
        // Query failures and partial masks above restore through the guard.
        Ok(guard.previous.take().expect("saved affinity mask"))
    }

    pub(super) fn restore(saved: &Saved) -> Result<(), AffinityError> {
        affinity::set_thread_affinity(saved).map_err(|e| AffinityError::Os(e.to_string()))
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Approved macOS-only Mach policy primitive.
mod backend {
    use super::AffinityError;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    pub(super) type Saved = libc::integer_t;

    fn check(result: libc::kern_return_t) -> Result<(), AffinityError> {
        match result {
            libc::KERN_SUCCESS => Ok(()),
            libc::KERN_NOT_SUPPORTED => Err(AffinityError::Unsupported),
            code => Err(AffinityError::Os(format!(
                "Mach affinity policy returned {code}"
            ))),
        }
    }

    fn tag() -> Result<Saved, AffinityError> {
        let mut policy = libc::thread_affinity_policy { affinity_tag: 0 };
        let mut count = libc::THREAD_AFFINITY_POLICY_COUNT;
        let mut get_default: libc::boolean_t = 0;
        // SAFETY: pthread_self identifies the live calling thread. Its Mach port
        // is borrowed (pthread_mach_thread_np does not create a new send right).
        // policy is a writable, initialized C-layout affinity policy; count
        // declares exactly its capacity in integer_t units. Both out-parameters
        // remain valid throughout the synchronous call.
        let result = unsafe {
            libc::thread_policy_get(
                libc::pthread_mach_thread_np(libc::pthread_self()),
                libc::THREAD_AFFINITY_POLICY as libc::thread_policy_flavor_t,
                (&mut policy as *mut libc::thread_affinity_policy).cast(),
                &mut count,
                &mut get_default,
            )
        };
        check(result)?;
        if count != libc::THREAD_AFFINITY_POLICY_COUNT {
            return Err(AffinityError::Os(
                "Mach returned an incomplete affinity policy".into(),
            ));
        }
        Ok(policy.affinity_tag)
    }

    pub(super) fn current() -> Result<Vec<usize>, AffinityError> {
        // No OS CPU mask exists for this advisory policy.
        Err(AffinityError::Unsupported)
    }

    pub(super) fn pin(cores: &[usize]) -> Result<Saved, AffinityError> {
        let count = std::thread::available_parallelism()
            .map_err(|e| AffinityError::Os(e.to_string()))?
            .get();
        if let Some(&id) = cores.iter().find(|&&id| id >= count) {
            return Err(AffinityError::InvalidCore(id));
        }
        let mut cores = cores.to_vec();
        cores.sort_unstable();
        cores.dedup();
        let mut hash = DefaultHasher::new();
        cores.hash(&mut hash);
        // A nonzero tag groups equal CPU sets; it never represents placement on
        // those CPUs. Zero is the OS's unhinted policy and must not be requested.
        let requested = (hash.finish() % i32::MAX as u64) as i32 + 1;
        let mut guard = super::AffinityGuard {
            previous: Some(tag()?),
            _thread_bound: std::marker::PhantomData,
        };
        restore(&requested)?;
        if tag()? != requested {
            return Err(AffinityError::Os(
                "Mach did not apply the requested affinity tag".into(),
            ));
        }
        Ok(guard.previous.take().expect("saved affinity tag"))
    }

    pub(super) fn restore(saved: &Saved) -> Result<(), AffinityError> {
        let mut policy = libc::thread_affinity_policy {
            affinity_tag: *saved,
        };
        // SAFETY: the port names the live calling thread, with no owned send
        // right to release. policy is a fully initialized C-layout policy, and
        // the matching count is in integer_t units. The OS consumes it during
        // this synchronous call; no pointer escapes.
        check(unsafe {
            libc::thread_policy_set(
                libc::pthread_mach_thread_np(libc::pthread_self()),
                libc::THREAD_AFFINITY_POLICY as libc::thread_policy_flavor_t,
                (&mut policy as *mut libc::thread_affinity_policy).cast(),
                libc::THREAD_AFFINITY_POLICY_COUNT,
            )
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn policy_round_trip_or_explicit_os_error() {
            // Some hardware rejects the hint. Assert either exact restoration or
            // an explicit error; never claim that a successful hint binds a CPU.
            match tag() {
                Ok(before) => match super::super::pin_current_thread(&[0]) {
                    Ok(guard) => {
                        assert_ne!(tag().unwrap(), 0);
                        drop(guard);
                        assert_eq!(tag().unwrap(), before);
                    }
                    Err(AffinityError::Os(_) | AffinityError::Unsupported) => {
                        assert_eq!(tag().unwrap(), before);
                    }
                    Err(error) => panic!("unexpected error: {error}"),
                },
                Err(AffinityError::Os(_) | AffinityError::Unsupported) => {
                    assert!(super::super::pin_current_thread(&[0]).is_err());
                }
                Err(error) => panic!("unexpected error: {error}"),
            }
            assert!(matches!(
                super::super::pin_current_thread(&[usize::MAX]),
                Err(AffinityError::InvalidCore(_))
            ));
            assert!(matches!(current(), Err(AffinityError::Unsupported)));
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod backend {
    use super::AffinityError;

    pub(super) type Saved = ();

    pub(super) fn current() -> Result<Vec<usize>, AffinityError> {
        Err(AffinityError::Unsupported)
    }

    pub(super) fn pin(_: &[usize]) -> Result<Saved, AffinityError> {
        Err(AffinityError::Unsupported)
    }

    pub(super) fn restore(_: &Saved) -> Result<(), AffinityError> {
        Ok(())
    }
}

fn parse_cpu_list(spec: &str) -> Result<Vec<usize>, AffinityError> {
    // Bound expansion before allocating. Backends validate individual CPU IDs.
    const MAX_ENTRIES: usize = 1024;
    let invalid = || AffinityError::InvalidSpecification(spec.into());
    let number = |text: &str| -> Result<usize, AffinityError> {
        let text = text.trim();
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        text.parse().map_err(|_| invalid())
    };
    let mut cores = Vec::new();
    for item in spec.split(',') {
        let (start, end) = match item.split_once('-') {
            Some((lo, hi)) => (number(lo)?, number(hi)?),
            None => {
                let id = number(item)?;
                (id, id)
            }
        };
        let count = end
            .checked_sub(start)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(invalid)?;
        if count > MAX_ENTRIES - cores.len() {
            return Err(invalid());
        }
        cores.extend(start..=end);
    }
    cores.sort_unstable();
    cores.dedup();
    Ok(cores)
}

pub(crate) fn configured_affinity() -> Result<Option<Vec<usize>>, AffinityError> {
    static CORES: OnceLock<Result<Option<Vec<usize>>, AffinityError>> = OnceLock::new();
    CORES
        .get_or_init(|| match std::env::var("ZENBENCH_PIN") {
            Ok(spec) => parse_cpu_list(&spec).map(Some),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(AffinityError::InvalidSpecification(
                "non-Unicode value".into(),
            )),
        })
        .clone()
}

thread_local! {
    static BENCHMARK_AFFINITY: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) };
}

pub(crate) fn benchmark_affinity() -> Option<Vec<usize>> {
    BENCHMARK_AFFINITY.with(|cores| cores.borrow().clone())
}

pub(crate) struct GroupAffinityGuard {
    _pin: Option<AffinityGuard>,
    previous: Option<Vec<usize>>,
}

impl GroupAffinityGuard {
    pub(crate) fn enter(cores: Option<&[usize]>) -> Result<Self, AffinityError> {
        let pin = cores.map(pin_current_thread).transpose()?;
        let previous = BENCHMARK_AFFINITY.with(|value| value.replace(cores.map(<[usize]>::to_vec)));
        Ok(Self {
            _pin: pin,
            previous,
        })
    }
}

impl Drop for GroupAffinityGuard {
    fn drop(&mut self) {
        BENCHMARK_AFFINITY.with(|value| value.replace(self.previous.take()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpu_lists() {
        assert_eq!(parse_cpu_list("2-7").unwrap(), [2, 3, 4, 5, 6, 7]);
        assert_eq!(parse_cpu_list(" 7, 2-4, 3, 0 ").unwrap(), [0, 2, 3, 4, 7]);
        assert_eq!(parse_cpu_list("0-0").unwrap(), [0]);
    }

    #[test]
    fn rejects_invalid_lists_and_unbounded_expansion() {
        for spec in [
            "",
            " ",
            "2-",
            "-2",
            "3-2",
            "1,,2",
            "1,",
            "one",
            "+1",
            "1-2-3",
            "0-999999999999999999999999",
            "0-1048576",
        ] {
            assert!(parse_cpu_list(spec).is_err(), "accepted {spec:?}");
        }
    }

    #[test]
    fn empty_set_is_an_error() {
        assert!(matches!(
            pin_current_thread(&[]),
            Err(AffinityError::EmptyCoreSet)
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn nested_pin_restores_previous_masks() {
        let before = current_thread_affinity().unwrap();
        let one = before[0];
        {
            let _outer = pin_current_thread(&[one]).unwrap();
            assert_eq!(current_thread_affinity().unwrap(), [one]);
            {
                let _inner = pin_current_thread(&before).unwrap();
                assert_eq!(current_thread_affinity().unwrap(), before);
            }
            assert_eq!(current_thread_affinity().unwrap(), [one]);
        }
        assert_eq!(current_thread_affinity().unwrap(), before);
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn invalid_cpu_does_not_change_affinity() {
        let before = current_thread_affinity().unwrap();
        assert!(matches!(
            pin_current_thread(&[usize::MAX]),
            Err(AffinityError::InvalidCore(_))
        ));
        assert_eq!(current_thread_affinity().unwrap(), before);
        // Linux must reject the whole request even when one CPU is usable.
        #[cfg(target_os = "linux")]
        {
            let offline = (0..libc::CPU_SETSIZE as usize)
                .find(|cpu| {
                    !std::path::Path::new(&format!("/sys/devices/system/cpu/cpu{cpu}")).exists()
                })
                .unwrap();
            assert!(pin_current_thread(&[before[0], offline]).is_err());
            assert_eq!(current_thread_affinity().unwrap(), before);
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn panic_restores_affinity() {
        let before = current_thread_affinity().unwrap();
        let result = std::panic::catch_unwind(|| {
            let _guard = pin_current_thread(&[before[0]]).unwrap();
            panic!("test unwind");
        });
        assert!(result.is_err());
        assert_eq!(current_thread_affinity().unwrap(), before);
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn worker_pin_failure_aborts_without_running_work_or_deadlocking() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let before = current_thread_affinity().unwrap();
        let _group = GroupAffinityGuard::enter(Some(&before)).unwrap();
        // Simulate a worker request rejected after coordinator admission.
        BENCHMARK_AFFINITY.with(|value| value.replace(Some(vec![usize::MAX])));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut group = crate::BenchGroup::new("worker failure");
        let counted = calls.clone();
        group.bench_parallel("parallel", 2, move |_, _| {
            counted.fetch_add(1, Ordering::Relaxed);
        });
        let counted = calls.clone();
        group.bench_contended(
            "contended",
            2,
            || (),
            move |_, _, _| {
                counted.fetch_add(1, Ordering::Relaxed);
            },
        );
        for benchmark in &mut group.benchmarks {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                benchmark.func.call(&mut crate::Bencher::new(1));
            }));
            assert!(result.is_err());
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(current_thread_affinity().unwrap(), before);
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    #[test]
    fn unsupported_target_is_an_error() {
        assert!(matches!(
            pin_current_thread(&[0]),
            Err(AffinityError::Unsupported)
        ));
    }
}
