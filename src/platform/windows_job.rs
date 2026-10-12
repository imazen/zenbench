//! An immutable, inherited process CPU boundary using the published safe API.
//! The caller serializes admission; the job remains alive for the process.
static JOB: std::sync::OnceLock<win32job::Job> = std::sync::OnceLock::new();

pub(super) fn installed() -> bool {
    JOB.get().is_some()
}

pub(super) fn install(cpus: &[usize]) -> Result<(), String> {
    if cpus.is_empty() || cpus.iter().any(|&c| c >= usize::BITS as usize) {
        return Err("invalid CPU affinity job mask".into());
    }
    if !installed() {
        let mask = cpus
            .iter()
            .fold(0usize, |mask, &cpu| mask | (1usize << cpu));
        let mut limits = win32job::ExtendedLimitInfo::new();
        limits.limit_affinity(mask);
        let job = win32job::Job::create_with_limit_info(&limits)
            .map_err(|e| format!("CPU affinity job creation failed: {e}"))?;
        job.assign_current_process()
            .map_err(|e| format!("CPU affinity job assignment failed: {e}"))?;
        JOB.set(job)
            .map_err(|_| "CPU affinity job already installed")?;
    }
    let actual = affinity::get_process_affinity().map_err(|e| e.to_string())?;
    if actual != cpus {
        return Err("CPU affinity job did not preserve the requested process boundary".into());
    }
    Ok(())
}
