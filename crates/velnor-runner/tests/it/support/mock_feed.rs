// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

const CLOSE_BUDGET: Duration = Duration::from_secs(10);

pub(crate) struct MockFeed {
    url: String,
    messages: Arc<Mutex<Vec<Value>>>,
    closed: Arc<Notify>,
    accept: JoinHandle<()>,
}

impl Drop for MockFeed {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

impl MockFeed {
    pub(crate) async fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind the feed server")?;
        let url = format!("http://{}/feed", listener.local_addr()?);
        let messages = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(Notify::new());
        let (seen, done) = (Arc::clone(&messages), Arc::clone(&closed));
        let accept = tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut socket) = accept_async(stream).await
            {
                while let Some(Ok(Message::Text(text))) = socket.next().await {
                    if let (Ok(frame), Ok(mut seen)) = (serde_json::from_str(&text), seen.lock()) {
                        seen.push(frame);
                    }
                }
            }
            done.notify_one();
        });
        Ok(Self {
            url,
            messages,
            closed,
            accept,
        })
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    pub(crate) async fn frames(&self) -> Result<Vec<Value>> {
        tokio::time::timeout(CLOSE_BUDGET, self.closed.notified())
            .await
            .context("the runner closed the feed connection")?;
        Ok(self
            .messages
            .lock()
            .map_err(|_| anyhow!("feed state poisoned"))?
            .clone())
    }
}
