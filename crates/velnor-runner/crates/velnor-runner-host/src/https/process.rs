use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use super::{CURL_ARGV, CurlFail, classify_exit};

pub(super) fn run_curl(
    config: &str,
    deadline: Instant,
    max_response_bytes: usize,
) -> Result<(u16, Vec<u8>), CurlFail> {
    run_curl_with_home(config, None, deadline, max_response_bytes)
}

#[cfg(test)]
pub(super) fn run_in_home(config: &str, home: &Path) -> Result<u16, CurlFail> {
    run_curl_with_home(
        config,
        Some(home),
        Instant::now() + Duration::from_secs(60),
        super::MAX_RESPONSE_BODY_BYTES,
    )
    .map(|(status, _)| status)
}

fn run_curl_with_home(
    config: &str,
    home: Option<&Path>,
    deadline: Instant,
    max_response_bytes: usize,
) -> Result<(u16, Vec<u8>), CurlFail> {
    if Instant::now() >= deadline {
        return Err(CurlFail::Timeout);
    }
    let mut command = Command::new("curl");
    command.args(CURL_ARGV);
    if let Some(home) = home {
        command.env("CURL_HOME", home).env("HOME", home);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| CurlFail::Reset)?;
    let Some(stdout) = child.stdout.take() else {
        stop_child(&mut child);
        return Err(CurlFail::Reset);
    };
    let Ok(reader) = thread::Builder::new()
        .name("velnor-https-response".to_owned())
        .spawn(move || read_bounded_response(stdout, max_response_bytes))
    else {
        stop_child(&mut child);
        return Err(CurlFail::Reset);
    };
    if let Err(error) = write_config_until(&mut child, config.as_bytes(), deadline) {
        stop_child(&mut child);
        let _reader_result = reader.join();
        return Err(error);
    }
    let finished = wait_until(child, deadline);
    let response = reader.join().map_err(|_| CurlFail::Reset)??;
    let finished = finished?;
    let status = classify_exit(finished.status.code(), &response.status)?;
    Ok((status, response.body))
}

struct BoundedResponse {
    body: Vec<u8>,
    status: Vec<u8>,
}

fn read_bounded_response<R: Read>(
    mut reader: R,
    max_body_bytes: usize,
) -> Result<BoundedResponse, CurlFail> {
    let mut body = Vec::with_capacity(max_body_bytes);
    let mut trailer = Vec::with_capacity(3);
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk).map_err(|_| CurlFail::Reset)?;
        if read == 0 {
            break;
        }
        trailer.extend_from_slice(&chunk[..read]);
        if trailer.len() > 3 {
            let body_bytes = trailer.len() - 3;
            if body.len().saturating_add(body_bytes) > max_body_bytes {
                return Err(CurlFail::ResponseTooLarge);
            }
            body.extend_from_slice(&trailer[..body_bytes]);
            trailer.drain(..body_bytes);
        }
    }
    Ok(BoundedResponse {
        body,
        status: trailer,
    })
}

fn write_config_until(child: &mut Child, config: &[u8], deadline: Instant) -> Result<(), CurlFail> {
    let Some(mut stdin) = child.stdin.take() else {
        stop_child(child);
        return Err(CurlFail::Reset);
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::scope(|scope| {
        scope.spawn(move || {
            let result = stdin.write_all(config);
            let _sent = sender.send(result);
        });
        loop {
            if Instant::now() >= deadline {
                stop_child(child);
                let _writer_result = receiver.recv();
                return Err(CurlFail::Timeout);
            }
            match receiver.try_recv() {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                    stop_child(child);
                    return Err(CurlFail::Reset);
                }
                Err(TryRecvError::Empty) => {}
            }
            match child.try_wait() {
                Ok(Some(_)) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    return match receiver.recv_timeout(left) {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            Err(CurlFail::Reset)
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => Err(CurlFail::Timeout),
                    };
                }
                Err(_) => {
                    stop_child(child);
                    let _writer_result = receiver.recv();
                    return Err(CurlFail::Reset);
                }
                Ok(None) => thread::sleep(Duration::from_millis(2)),
            }
        }
    })
}

fn wait_until(mut child: Child, deadline: Instant) -> Result<std::process::Output, CurlFail> {
    loop {
        if Instant::now() >= deadline {
            stop_child(&mut child);
            return Err(CurlFail::Timeout);
        }
        match child.try_wait() {
            Err(_) => {
                stop_child(&mut child);
                return Err(CurlFail::Reset);
            }
            Ok(Some(_)) => return child.wait_with_output().map_err(|_| CurlFail::Reset),
            Ok(None) => thread::sleep(Duration::from_millis(2)),
        }
    }
}

fn stop_child(child: &mut Child) {
    let _killed = child.kill();
    let _reaped = child.wait();
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::{CurlFail, wait_until, write_config_until};

    #[test]
    fn deadline_stops_and_reaps_a_waiting_child() {
        let child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start child");
        let began = Instant::now();

        assert!(matches!(
            wait_until(child, began + Duration::from_millis(80)),
            Err(CurlFail::Timeout)
        ));
        assert!(began.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn deadline_stops_a_child_that_does_not_read_its_config() {
        let mut child = Command::new("sleep")
            .arg("5")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start child");
        let config = vec![b'x'; 1024 * 1024];
        let began = Instant::now();

        assert!(matches!(
            write_config_until(&mut child, &config, began + Duration::from_millis(80)),
            Err(CurlFail::Timeout)
        ));
        assert!(began.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn quick_reader_can_exit_after_the_config_write() {
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start child");
        let config = b"complete request";
        let deadline = Instant::now() + Duration::from_secs(2);

        assert!(matches!(
            write_config_until(&mut child, config, deadline),
            Ok(())
        ));
        assert!(matches!(
            wait_until(child, deadline),
            Ok(output) if output.stdout == config
        ));
    }
}
