use super::capture_output;
use std::io::{self, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("injected reader failure"))
    }
}

struct InterruptedReader {
    calls: Arc<AtomicUsize>,
}

impl Read for InterruptedReader {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        let calls = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        if calls > super::super::process::INTERRUPTED_READ_LIMIT + 1 {
            Ok(0)
        } else {
            Err(io::Error::from(io::ErrorKind::Interrupted))
        }
    }
}

#[test]
fn output_capture_propagates_injected_read_error() -> io::Result<()> {
    let output = Arc::new(Mutex::new(Vec::new()));
    let Err(error) = capture_output(FailingReader, output.as_ref()) else {
        return Err(io::Error::other("reader error did not propagate"));
    };

    assert!(error.to_string().contains("injected reader failure"));
    Ok(())
}

#[test]
fn output_capture_bounds_repeated_interrupted_reads() -> io::Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let reader = InterruptedReader {
        calls: Arc::clone(&calls),
    };
    let output = Arc::new(Mutex::new(Vec::new()));
    let Err(error) = capture_output(reader, output.as_ref()) else {
        return Err(io::Error::other("repeated interrupts did not fail"));
    };

    assert!(error.to_string().contains("repeatedly interrupted"));
    assert_eq!(
        calls.load(Ordering::Relaxed),
        super::super::process::INTERRUPTED_READ_LIMIT
    );
    Ok(())
}
