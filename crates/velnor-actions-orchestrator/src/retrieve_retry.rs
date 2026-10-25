pub(crate) const MAX_DOWNLOAD_ATTEMPTS: u32 = 3;

pub(crate) fn download_with_retry(mut attempt: impl FnMut() -> bool) -> (bool, u32) {
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        if attempt() {
            return (true, attempts);
        }
        if attempts >= MAX_DOWNLOAD_ATTEMPTS {
            return (false, attempts);
        }
    }
}
