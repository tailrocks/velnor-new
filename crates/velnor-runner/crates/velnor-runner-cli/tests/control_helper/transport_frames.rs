use std::error::Error;
use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};

use super::{OwnedState, accept, read_request, run_control};

#[test]
fn fragmented_boolean_response_survives_immediate_peer_close() -> Result<(), Box<dyn Error>> {
    let state = OwnedState::new()?;
    let listener = state.listener()?;
    let peer = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut stream = accept(&listener)?;
        read_request(&mut stream)?;
        let response = br#"{"value":true}"#;
        let prefix = u32::try_from(response.len())?.to_be_bytes();
        stream.write_all(&prefix[..2])?;
        thread::sleep(Duration::from_millis(10));
        stream.write_all(&prefix[2..])?;
        for part in response.chunks(3) {
            stream.write_all(part)?;
            thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    });

    let started = Instant::now();
    let output = run_control(state.path(), 1_000, false)?;
    peer.join()
        .map_err(|_| "peer panicked")?
        .map_err(|error| error.to_string())?;

    assert!(output.status.success());
    assert_eq!(output.stdout, b"true\n");
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn partial_response_eof_fails_without_a_boolean_result() -> Result<(), Box<dyn Error>> {
    let state = OwnedState::new()?;
    let listener = state.listener()?;
    let peer = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut stream = accept(&listener)?;
        read_request(&mut stream)?;
        stream.write_all(&16_u32.to_be_bytes())?;
        stream.write_all(br#"{"value""#)?;
        Ok(())
    });

    let started = Instant::now();
    let output = run_control(state.path(), 1_000, false)?;
    peer.join()
        .map_err(|_| "peer panicked")?
        .map_err(|error| error.to_string())?;

    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}
