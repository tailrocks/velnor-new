use std::process::{Child, ExitStatus};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use velnor_runner_github::TransportFail;
use zeroize::Zeroize;

use super::super::readers::BodyReadError;

pub(super) fn wait_for_exit(
    child: &mut Child,
    body_rx: &Receiver<Result<Vec<u8>, BodyReadError>>,
    config_rx: &Receiver<Result<(), ()>>,
    config_written: &mut bool,
    stop_at: Instant,
    cancellation: &AtomicBool,
) -> Result<(ExitStatus, Option<Vec<u8>>), TransportFail> {
    let mut body = None;
    loop {
        if cancellation.load(Ordering::Acquire) {
            zeroize_body(&mut body);
            return Err(TransportFail::Reset);
        }
        if Instant::now() >= stop_at {
            zeroize_body(&mut body);
            return Err(TransportFail::Timeout);
        }
        if !*config_written {
            match config_rx.try_recv() {
                Ok(Ok(())) => *config_written = true,
                Ok(Err(())) | Err(mpsc::TryRecvError::Disconnected) => {
                    zeroize_body(&mut body);
                    return Err(TransportFail::Reset);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if body.is_none() {
            match body_rx.try_recv() {
                Ok(Ok(bytes)) => body = Some(bytes),
                Ok(Err(BodyReadError::TooLarge | BodyReadError::Io))
                | Err(mpsc::TryRecvError::Disconnected) => {
                    zeroize_body(&mut body);
                    return Err(TransportFail::Reset);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !*config_written
                    && let Err(error) =
                        wait_for_config(config_rx, config_written, stop_at, cancellation)
                {
                    zeroize_body(&mut body);
                    return Err(error);
                }
                return Ok((status, body));
            }
            Ok(None) => {}
            Err(_) => {
                zeroize_body(&mut body);
                return Err(TransportFail::Reset);
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn zeroize_body(body: &mut Option<Vec<u8>>) {
    if let Some(mut body) = body.take() {
        body.zeroize();
    }
}

fn wait_for_config(
    receiver: &Receiver<Result<(), ()>>,
    config_written: &mut bool,
    stop_at: Instant,
    cancellation: &AtomicBool,
) -> Result<(), TransportFail> {
    while !*config_written {
        if cancellation.load(Ordering::Acquire) {
            return Err(TransportFail::Reset);
        }
        if Instant::now() >= stop_at {
            return Err(TransportFail::Timeout);
        }
        match receiver.try_recv() {
            Ok(Ok(())) => *config_written = true,
            Ok(Err(())) | Err(mpsc::TryRecvError::Disconnected) => {
                return Err(TransportFail::Reset);
            }
            Err(mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
        }
    }
    Ok(())
}

pub(super) fn receive_result(
    receiver: &Receiver<Result<Vec<u8>, BodyReadError>>,
    stop_at: Instant,
    cancellation: &AtomicBool,
) -> Result<Vec<u8>, TransportFail> {
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(TransportFail::Reset);
        }
        if Instant::now() >= stop_at {
            return Err(TransportFail::Timeout);
        }
        match receiver.try_recv() {
            Ok(Ok(bytes)) if Instant::now() < stop_at => return Ok(bytes),
            Ok(Ok(mut bytes)) => {
                bytes.zeroize();
                return Err(TransportFail::Timeout);
            }
            Ok(Err(BodyReadError::TooLarge | BodyReadError::Io))
            | Err(mpsc::TryRecvError::Disconnected) => return Err(TransportFail::Reset),
            Err(mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
        }
    }
}

pub(super) fn thread_done(thread: Option<&JoinHandle<()>>) -> bool {
    thread.is_none_or(JoinHandle::is_finished)
}

pub(super) fn join_finished(thread: &mut Option<JoinHandle<()>>) -> Result<bool, TransportFail> {
    if let Some(handle) = thread.as_ref()
        && !handle.is_finished()
    {
        return Ok(false);
    }
    if let Some(handle) = thread.take() {
        handle.join().map_err(|_| TransportFail::Reset)?;
    }
    Ok(true)
}
