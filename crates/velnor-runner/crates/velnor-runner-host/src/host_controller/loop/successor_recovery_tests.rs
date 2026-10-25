#[path = "successor_recovery_tests/controller.rs"]
mod controller;
#[path = "successor_recovery_tests/scenario.rs"]
mod scenario;

#[test]
fn max_jobs_one_restarts_same_acquire_on_a_fresh_physical_attempt() -> Result<(), String> {
    const TEST_HARNESS_STACK_BYTES: usize = 8 * 1024 * 1024;
    std::thread::Builder::new()
        .name("successor-test-harness-stack".to_owned())
        .stack_size(TEST_HARNESS_STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(scenario::max_jobs_one_successor())
        })
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "The successor recovery test thread panicked.".to_owned())?
}
