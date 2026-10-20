struct CacheLock {
    _file: File,
}

impl CacheLock {
    async fn acquire(root: &Path, wait: Duration) -> io::Result<Self> {
        validate_private_directory_path(root)?;
        let lock_path = root.join(LOCK_FILE);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let file = match options.open(&lock_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let mut existing = OpenOptions::new();
                existing.read(true).write(true);
                #[cfg(unix)]
                existing.custom_flags(libc::O_NOFOLLOW);
                existing.open(&lock_path)?
            }
            Err(error) => return Err(error),
        };
        reject_regular_private(&file)?;
        let deadline = Instant::now() + wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "TUF cache lock timed out",
                        ));
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(error),
            }
        }
    }
}
