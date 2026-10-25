use std::{future::Future, pin::Pin};

const TEST_HARNESS_STACK_BYTES: usize = 8 * 1024 * 1024;

pub(crate) fn run_on_test_harness_stack<F>(
    name: &'static str,
    future_factory: F,
) -> Result<(), String>
where
    F: FnOnce() -> Pin<Box<dyn Future<Output = Result<(), String>>>> + Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_owned())
        .stack_size(TEST_HARNESS_STACK_BYTES)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(future_factory())
        })
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "The test failed.".to_owned())?
}
