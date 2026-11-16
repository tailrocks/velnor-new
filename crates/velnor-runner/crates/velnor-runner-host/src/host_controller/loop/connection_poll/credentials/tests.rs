// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::restore_missing;

struct Connection {
    credentials: Option<u32>,
    restores: u32,
    recoveries: u32,
}

impl Connection {
    async fn poll(&mut self, fail: bool) -> Result<(), ()> {
        let restores = &mut self.restores;
        restore_missing(&mut self.credentials, async || {
            *restores += 1;
            Ok::<_, ()>(7)
        })
        .await?;
        if fail {
            self.credentials.take();
            return Err(());
        }
        self.recoveries += 1;
        Ok(())
    }
}

#[tokio::test]
async fn a_held_poll_does_not_strand_the_next_poll() {
    let mut connection = Connection {
        credentials: Some(7),
        restores: 0,
        recoveries: 0,
    };
    assert!(connection.poll(true).await.is_err());
    assert!(connection.credentials.is_none());
    assert!(connection.poll(false).await.is_ok());
    assert_eq!(connection.restores, 1);
    assert_eq!(connection.recoveries, 1);
    assert!(connection.credentials.is_some());
}

#[tokio::test]
async fn present_credentials_are_not_reloaded() {
    let mut slot = Some(1_u32);
    restore_missing(&mut slot, async || -> Result<u32, ()> { Err(()) })
        .await
        .unwrap();
    assert_eq!(slot, Some(1));
}

#[tokio::test]
async fn a_failed_restore_keeps_the_poll_held() {
    let mut slot: Option<u32> = None;
    assert!(
        restore_missing(&mut slot, async || -> Result<u32, ()> { Err(()) })
            .await
            .is_err()
    );
    assert!(slot.is_none());
}
