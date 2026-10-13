use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;
use velnor_runner_github::{DiscoveryExchange, Exchange, SessionRequest, TransportFail};
use zeroize::{Zeroize, Zeroizing};

use super::BoundedDiscoveryTransport;

fn request_stop_at(
    started_at: Instant,
    request_deadline: Duration,
    absolute_cutoff: Option<Instant>,
) -> Option<Instant> {
    let request_cutoff = started_at.checked_add(request_deadline)?;
    Some(absolute_cutoff.map_or(request_cutoff, |cutoff| cutoff.min(request_cutoff)))
}

impl BoundedDiscoveryTransport {
    /// Run one synchronous HTTP exchange under both the ordinary request cap
    /// and the caller's retained absolute operation cutoff.
    ///
    /// `None` keeps the usual per-request limit. Callers performing refreshes
    /// or replays should pass the same absolute cutoff on every exchange.
    ///
    /// # Errors
    ///
    /// Returns `Timeout` when either deadline expires and `Reset` when
    /// cancellation is requested or the bounded transport cannot dispatch.
    pub fn exchange_until(
        &mut self,
        request: &SessionRequest,
        absolute_cutoff: Option<Instant>,
        cancellation: &AtomicBool,
    ) -> Result<Exchange, TransportFail> {
        if cancellation.load(Ordering::Acquire) {
            return Err(TransportFail::Reset);
        }
        let started_at = Instant::now();
        let stop_at = request_stop_at(started_at, self.request_deadline, absolute_cutoff)
            .ok_or(TransportFail::Timeout)?;
        if started_at >= stop_at {
            return Err(TransportFail::Timeout);
        }
        let url = self.request_url(request)?;
        if cancellation.load(Ordering::Acquire) {
            return Err(TransportFail::Reset);
        }
        if Instant::now() >= stop_at {
            return Err(TransportFail::Timeout);
        }

        let permit = Arc::new(super::try_discovery_worker_permit()?);
        let result = super::curl::perform_curl_until_cancellable_with_permit(
            &self.curl,
            url.as_str(),
            request,
            self.response_limit,
            stop_at,
            cancellation,
            permit,
        );
        match result {
            Ok(mut exchange) if cancellation.load(Ordering::Acquire) => {
                exchange.body.zeroize();
                Err(TransportFail::Reset)
            }
            Ok(mut exchange) if Instant::now() >= stop_at => {
                exchange.body.zeroize();
                Err(TransportFail::Timeout)
            }
            result => result,
        }
    }

    /// Start one asynchronous HTTP exchange using the caller's absolute
    /// operation cutoff and cancellation flag. The deadline is computed before
    /// route validation and is not reset by a later auth refresh or replay.
    ///
    /// # Errors
    ///
    /// The future returns `Timeout` when a deadline expires and `Reset` when
    /// cancellation is requested or bounded worker admission is unavailable.
    pub fn exchange_discovery_until(
        &mut self,
        request: SessionRequest,
        absolute_cutoff: Option<Instant>,
        cancellation: Arc<AtomicBool>,
    ) -> DiscoveryExchange {
        if cancellation.load(Ordering::Acquire) {
            return failed_exchange(TransportFail::Reset, cancellation);
        }
        let started_at = Instant::now();
        let stop_at = match request_stop_at(started_at, self.request_deadline, absolute_cutoff) {
            Some(stop_at) if started_at < stop_at => stop_at,
            Some(_) | None => return failed_exchange(TransportFail::Timeout, cancellation),
        };
        let prepared = self.request_url(&request).and_then(|url| {
            if cancellation.load(Ordering::Acquire) {
                return Err(TransportFail::Reset);
            }
            if Instant::now() >= stop_at {
                return Err(TransportFail::Timeout);
            }
            Ok(url)
        });
        let url = match prepared {
            Ok(url) => url,
            Err(error) => return failed_exchange(error, cancellation),
        };

        let executable = self.curl.clone();
        let response_limit = self.response_limit;
        let worker_cancellation = Arc::clone(&cancellation);
        DiscoveryExchange::new(
            async move {
                perform_async_curl(
                    executable,
                    url,
                    request,
                    response_limit,
                    stop_at,
                    worker_cancellation,
                )
                .await
            },
            cancellation,
        )
    }
}

async fn perform_async_curl(
    executable: String,
    url: Zeroizing<String>,
    request: SessionRequest,
    response_limit: usize,
    stop_at: Instant,
    cancellation: Arc<AtomicBool>,
) -> Result<Exchange, TransportFail> {
    if cancellation.load(Ordering::Acquire) {
        return Err(TransportFail::Reset);
    }
    if Instant::now() >= stop_at {
        return Err(TransportFail::Timeout);
    }
    let permit = super::try_discovery_worker_permit()?;
    let (sender, receiver) = oneshot::channel();
    let thread_cancellation = Arc::clone(&cancellation);
    let worker = thread::Builder::new()
        .name("velnor-discovery-curl".to_owned())
        .spawn(move || {
            let permit = Arc::new(permit);
            let result = super::curl::perform_curl_until_cancellable_with_permit(
                &executable,
                url.as_str(),
                &request,
                response_limit,
                stop_at,
                &thread_cancellation,
                permit,
            );
            if let Err(mut result) = sender.send(result)
                && let Ok(exchange) = &mut result
            {
                exchange.body.zeroize();
            }
        })
        .map_err(|_| TransportFail::Reset)?;
    let result = receiver.await.map_err(|_| TransportFail::Reset)?;
    if worker.join().is_err() {
        return Err(TransportFail::Reset);
    }
    match result {
        Ok(mut exchange) if cancellation.load(Ordering::Acquire) => {
            exchange.body.zeroize();
            Err(TransportFail::Reset)
        }
        Ok(mut exchange) if Instant::now() >= stop_at => {
            exchange.body.zeroize();
            Err(TransportFail::Timeout)
        }
        result => result,
    }
}

fn failed_exchange(error: TransportFail, cancellation: Arc<AtomicBool>) -> DiscoveryExchange {
    DiscoveryExchange::new(async move { Err(error) }, cancellation)
}
