use crate::error::HostError;
use crate::journal::Journal;

use super::prepare_forged_progress_case;

#[tokio::test]
async fn reopen_rejects_forged_progress_evidence() -> Result<(), HostError> {
    let (path, scratch) = prepare_forged_progress_case('e', 825, 1_125, false, &[1]).await?;
    assert!(Journal::open(&path).await.is_err());
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn reopen_rejects_forged_progress_evidence_remote() -> Result<(), HostError> {
    let (path, scratch) = prepare_forged_progress_case('f', 826, 1_126, true, &[2]).await?;
    assert!(Journal::open(&path).await.is_err());
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn reopen_rejects_forged_progress_evidence_matching() -> Result<(), HostError> {
    let (path, scratch) = prepare_forged_progress_case('0', 827, 1_127, true, &[1, 2]).await?;
    assert!(Journal::open(&path).await.is_err());
    drop(scratch);
    Ok(())
}
