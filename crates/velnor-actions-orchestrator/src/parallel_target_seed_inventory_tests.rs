use super::*;

use std::io;
use std::process::Command;

struct GrowingReader {
    reads: usize,
}

impl Read for GrowingReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let bytes: &[u8] = match self.reads {
            0 => b"abc",
            1 => b"d",
            _ => return Ok(0),
        };
        self.reads += 1;
        output[..bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
    }
}

#[test]
fn growth_during_copy_never_writes_beyond_the_admitted_size() {
    let snapshot = Snapshot::new(
        Instant::now(),
        SeedLimits {
            seconds: Duration::from_secs(1),
            bytes: 16,
            entries: 8,
            reserve: 0,
        },
    );
    let mut input = GrowingReader { reads: 0 };
    let mut output = Vec::new();

    assert!(matches!(
        copy_bounded(&snapshot, &mut input, &mut output, 3),
        Err(SeedError::Contract(_))
    ));
    assert_eq!(output, b"abc");
    assert!(output.len() <= 3);
}

#[test]
fn regular_to_fifo_swap_is_opened_nonblocking_and_rejected() {
    let root = std::env::temp_dir().join(format!(
        "velnor-parallel-seed-fifo-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("temporary root");
    let path = root.join("source");
    fs::write(&path, b"baseline").expect("regular source");
    let expected = Identity::from_metadata(&fs::metadata(&path).expect("source metadata"));
    let parent = super::super::filesystem::open_dir_path(&root).expect("source directory fd");
    fs::remove_file(&path).expect("remove regular source");
    let status = Command::new("mkfifo").arg(&path).status().expect("mkfifo");
    assert!(status.success(), "mkfifo returned {status}");

    assert!(open_copy_source(&parent, std::ffi::OsStr::new("source"), expected).is_err());

    drop(parent);
    fs::remove_dir_all(root).expect("remove temporary root");
}

#[test]
fn raw_mode_conversion_accepts_platform_width_and_rejects_narrowing() {
    assert_eq!(checked_raw_mode::<u16>(0o750).expect("mode"), 0o750);
    assert!(checked_raw_mode::<u16>(u32::MAX).is_err());
    assert_eq!(
        checked_raw_mode::<u32>(u32::MAX).expect("wide mode"),
        u32::MAX
    );
}

#[test]
fn timestamp_conversion_preserves_pre_epoch_time_and_rejects_invalid_nanos() {
    let expected = UNIX_EPOCH
        .checked_sub(Duration::from_nanos(1))
        .expect("representable timestamp");
    assert_eq!(
        system_time((-1, 999_999_999)).expect("pre-epoch time"),
        expected
    );
    assert!(system_time((0, -1)).is_err());
    assert!(system_time((0, 1_000_000_000)).is_err());
}
