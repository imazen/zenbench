//! Allocation/driver-protocol control; not a performance benchmark.
#![forbid(unsafe_code)]
fn main() {
    let bytes = vec![7u8; 1024 * 1024];
    assert_eq!(std::hint::black_box(&bytes)[1024], 7);
    drop(bytes);
    let path = std::env::var_os("ZENBENCH_RESULT_FILE").expect("runner result path");
    std::fs::write(
        path,
        r#"{"schema":1,"operations":1,"correctness":"ok","memory":[]}"#,
    )
    .unwrap();
}
