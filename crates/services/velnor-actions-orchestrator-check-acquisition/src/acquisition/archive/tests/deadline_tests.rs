use super::*;

use std::io::{Cursor, Read, Write};
use std::time::Duration;

#[test]
fn tar_preflight_checks_the_shared_deadline_during_decoded_reads() {
    struct SlowArchive;

    impl Read for SlowArchive {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(20));
            buffer.fill(0);
            Ok(buffer.len())
        }
    }

    let deadline = CheckDeadline::after(Duration::from_millis(5)).expect("deadline");
    let error = tar_preflight::preflight_tar(DeadlineIo::new(SlowArchive, deadline))
        .expect_err("preflight cannot outlive the check");
    assert!(matches!(error, OrchestratorError::Internal { .. }));
    assert!(deadline.remaining().is_err());
}

#[test]
fn buffered_gzip_output_checks_deadline_after_compressed_input_is_read() {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&vec![b'x'; 16 * 1024])
        .expect("gzip fixture");
    let compressed = encoder.finish().expect("gzip finish");
    let deadline = CheckDeadline::after(Duration::from_millis(50)).expect("deadline");
    let mut decoded = gzip_reader(Cursor::new(compressed), deadline);
    let mut first = [0_u8; 512];
    decoded
        .read_exact(&mut first)
        .expect("initial decoded chunk");
    std::thread::sleep(Duration::from_millis(60));
    let error = decoded
        .read(&mut first)
        .expect_err("buffered decoded bytes still observe the shared deadline");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(deadline.remaining().is_err());
}
