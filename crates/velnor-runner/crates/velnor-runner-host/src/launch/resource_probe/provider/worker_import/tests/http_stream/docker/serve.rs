use tokio::net::UnixStream;
use tokio::sync::oneshot;

use super::protocol::{read_first_chunk, read_head, read_request_body, write_response};
use super::{CapturedRequest, ImportReply, Reply};

pub(super) async fn serve_request(
    stream: &mut UnixStream,
    reply: Reply,
    stopped: &mut oneshot::Receiver<()>,
) -> Result<(CapturedRequest, bool), String> {
    let head = read_head(stream).await.map_err(|error| error.to_string())?;
    let mut captured = CapturedRequest {
        line: head.line.clone(),
        body: Vec::new(),
        body_complete: false,
        state_at_import: None,
    };
    match reply {
        Reply::Http { status, body } => {
            captured.body_complete = read_request_body(stream, &head, &mut captured.body)
                .await
                .unwrap_or(false);
            write_response(stream, status, &body)
                .await
                .map_err(|error| error.to_string())?;
            Ok((captured, false))
        }
        Reply::Import { mode, state_probe } => {
            let (journal, request) = *state_probe;
            let record = journal
                .prepare_image_import(request)
                .await
                .map_err(|error| error.to_string())?;
            captured.state_at_import = Some(record.state);
            serve_import(stream, &head, captured, mode, stopped).await
        }
    }
}

async fn serve_import(
    stream: &mut UnixStream,
    head: &super::protocol::RequestHead,
    mut captured: CapturedRequest,
    mode: ImportReply,
    stopped: &mut oneshot::Receiver<()>,
) -> Result<(CapturedRequest, bool), String> {
    match mode {
        ImportReply::Complete { status, body } => {
            captured.body_complete = read_request_body(stream, head, &mut captured.body)
                .await
                .unwrap_or(false);
            write_response(stream, status, &body)
                .await
                .map_err(|error| error.to_string())?;
            Ok((captured, false))
        }
        ImportReply::Early { status, body } => {
            write_response(stream, status, &body)
                .await
                .map_err(|error| error.to_string())?;
            Ok((captured, false))
        }
        ImportReply::Truncated => {
            let _has_prefix = read_first_chunk(stream, head, &mut captured.body)
                .await
                .unwrap_or(false);
            Ok((captured, false))
        }
        ImportReply::DisconnectAfterBody => {
            captured.body_complete = read_request_body(stream, head, &mut captured.body)
                .await
                .unwrap_or(false);
            Ok((captured, false))
        }
        ImportReply::Hang => {
            tokio::select! {
                _ = stopped => Ok((captured, true)),
                () = std::future::pending::<()>() => unreachable!(),
            }
        }
    }
}
