// Test helper: acquire an fs4 exclusive lock on the given path, signal
// readiness on stdout, then sleep until killed. Used by the
// crash-stale-lock integration test to verify the kernel releases the
// flock when the holder dies.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use std::io::Write;

    let path = std::env::args().nth(1).expect("usage: lock_holder <path>");

    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(&path)
        .expect("open lock file");

    fs4::FileExt::lock(&file).expect("acquire exclusive lock");

    let mut stdout = std::io::stdout();
    writeln!(stdout, "LOCKED {}", std::process::id()).unwrap();
    stdout.flush().unwrap();

    // Park forever — parent will SIGKILL/TerminateProcess us.
    std::thread::sleep(std::time::Duration::from_secs(3600));
}

#[cfg(target_arch = "wasm32")]
fn main() {}
