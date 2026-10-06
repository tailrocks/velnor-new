use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{HostError, connect_unix, start_pair};

struct IdleDocker {
    path: PathBuf,
    docker: bollard::Docker,
}

impl IdleDocker {
    fn open() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let tick = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Docker)?
            .as_nanos();
        let path = PathBuf::from(format!(
            "/tmp/velnor-w-{}-{n}-{tick}.sock",
            std::process::id()
        ));
        let listener =
            std::os::unix::net::UnixListener::bind(&path).map_err(|_| HostError::Docker)?;
        let text = path.to_str().ok_or(HostError::Path)?;
        let docker = connect_unix(text)?;
        drop(listener);
        Ok(Self { path, docker })
    }
}

impl Drop for IdleDocker {
    fn drop(&mut self) {
        let removed = std::fs::remove_file(&self.path);
        let _kept = removed.err().map(|err| err.kind());
    }
}

#[tokio::test]
async fn empty_jit_does_not_create() -> Result<(), HostError> {
    let idle = IdleDocker::open()?;
    assert_eq!(
        start_pair(&idle.docker, "a/b", b"").await,
        Err(HostError::EmptyJit)
    );
    assert_eq!(
        start_pair(&idle.docker, "worker_a", b"").await,
        Err(HostError::EmptyJit)
    );
    Ok(())
}
