use flate2::{Decompress, FlushDecompress, Status};

/// Return true only when one raw DEFLATE stream ends at the declared boundary.
pub(super) fn exact_consumption(compressed: &[u8], uncompressed_size: u32) -> bool {
    let Some(output_size) = usize::try_from(uncompressed_size)
        .ok()
        .and_then(|size| size.checked_add(1))
    else {
        return false;
    };
    let mut output = vec![0; output_size];
    let mut decoder = Decompress::new(false);
    matches!(
        decoder.decompress(compressed, &mut output, FlushDecompress::Finish),
        Ok(Status::StreamEnd)
    ) && usize::try_from(decoder.total_in()).ok() == Some(compressed.len())
        && decoder.total_out() == u64::from(uncompressed_size)
}
