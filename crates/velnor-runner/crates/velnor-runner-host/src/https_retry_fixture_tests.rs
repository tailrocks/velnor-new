use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use super::{RetryFixture, is_socket_timeout};

pub(super) fn read_headers_until(stream: &mut TcpStream, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    let header_error = || super::deadline_error("request headers exceeded their deadline");
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(header_error());
        }
        stream.set_read_timeout(Some(remaining))?;
        let count = match stream.read(&mut chunk) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if is_socket_timeout(&error) && Instant::now() >= deadline => {
                return Err(header_error());
            }
            Err(error) if is_socket_timeout(&error) => continue,
            result => result?,
        };
        if Instant::now() >= deadline {
            return Err(header_error());
        }
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > 16 * 1024 {
            return Err(io::Error::other("request headers exceeded bound"));
        }
    }
    Ok(())
}

#[test]
fn retry_fixture_finishes_when_no_client_connects() -> io::Result<()> {
    let started = Instant::now();
    let fixture = RetryFixture::start_with_first_accept_timeout(Duration::from_millis(40))?;
    let error = fixture
        .finish()
        .expect_err("fixture without a client must time out waiting for the first request");

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "fixture waited too long for a client"
    );
    Ok(())
}

#[test]
fn partial_request_headers_cannot_extend_the_read_deadline() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let mut writer = TcpStream::connect(listener.local_addr()?)?;
    let (mut reader, _) = listener.accept()?;
    let writer_thread = thread::spawn(move || -> io::Result<()> {
        writer.set_write_timeout(Some(Duration::from_secs(2)))?;
        for _ in 0..20 {
            writer.write_all(b"x")?;
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    });

    let result = read_headers_until(&mut reader, Duration::from_millis(150));
    writer_thread
        .join()
        .map_err(|_| io::Error::other("partial-header writer panicked"))??;
    let error = result.expect_err("partial headers must not reset their absolute deadline");

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    Ok(())
}
