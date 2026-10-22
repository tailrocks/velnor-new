use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

pub(super) use super::http::write_response;

pub(super) async fn record_jit_input(
    stream: &mut UnixStream,
    jit_input: &std::sync::Mutex<Vec<u8>>,
) -> Result<(), String> {
    stream
        .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
        .await
        .map_err(|error| error.to_string())?;
    let mut payload = Vec::new();
    stream
        .read_to_end(&mut payload)
        .await
        .map_err(|error| error.to_string())?;
    jit_input
        .lock()
        .map_err(|_| "The JIT configuration lock failed.".to_owned())?
        .extend(payload);
    Ok(())
}
