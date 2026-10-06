//! Bounded per-leg fetch retry (F5, split from `retrieve_reports`).
//!
//! Transient `gh` failures retry up to [`MAX_DOWNLOAD_ATTEMPTS`];
//! persistent failures skip the leg (the merge judges `not_run`).

/// Bounded per-artifact download attempts (F5).
///
/// Transient `gh` failures retry up to this many attempts per leg;
/// persistent failures skip the leg (the merge judges `not_run`).
/// Replaces the old `continue-on-error` fetch tail: retries absorb
/// flakes, while hard environment failures fail the job instead of
/// being masked.
pub(crate) const MAX_DOWNLOAD_ATTEMPTS: u32 = 3;

/// Attempt one download up to the bounded retry limit.
///
/// Returns success plus the attempts spent (1 on first-try success,
/// [`MAX_DOWNLOAD_ATTEMPTS`] on persistent failure). Pure over the
/// attempt closure so the bound is unit-testable without `gh`.
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

#[cfg(test)]
mod tests;
