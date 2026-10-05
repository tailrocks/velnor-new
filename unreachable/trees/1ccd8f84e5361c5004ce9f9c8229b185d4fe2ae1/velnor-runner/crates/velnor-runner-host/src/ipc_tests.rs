//! Length-prefixed frames. The prefix is big-endian.

use crate::{HostError, MAX_FRAME, decode_frame, encode_frame};

#[test]
fn roundtrip_is_big_endian() -> Result<(), HostError> {
    let payload = [0_u8, 1, 2, 255];
    let frame = encode_frame(&payload)?;
    assert_eq!(&frame[..4], &4_u32.to_be_bytes());
    assert_eq!(decode_frame(&frame)?, &payload);
    let one = encode_frame(&[0xAB])?;
    assert_eq!(one, vec![0x00, 0x00, 0x00, 0x01, 0xAB]);
    assert_eq!(decode_frame(&one)?, &[0xAB]);
    let empty = encode_frame(b"")?;
    assert_eq!(empty, vec![0, 0, 0, 0]);
    assert_eq!(decode_frame(&empty)?, b"");
    let mut trailed = encode_frame(b"hi")?;
    trailed.push(0xFF);
    assert_eq!(decode_frame(&trailed)?, b"hi");
    Ok(())
}

#[test]
fn oversize_and_truncated_frames_are_rejected() -> Result<(), HostError> {
    assert_eq!(MAX_FRAME, 1024 * 1024);
    let mut big = vec![0_u8; MAX_FRAME];
    big.push(1);
    assert_eq!(encode_frame(&big), Err(HostError::Frame));
    let exact = vec![7_u8; MAX_FRAME];
    let encoded = encode_frame(&exact)?;
    assert_eq!(decode_frame(&encoded)?, exact.as_slice());
    let len = u32::try_from(MAX_FRAME).map_err(|_| HostError::Frame)?;
    let mut prefix = len.saturating_add(1).to_be_bytes().to_vec();
    prefix.extend_from_slice(&[0, 0]);
    assert_eq!(decode_frame(&prefix), Err(HostError::Frame));
    assert_eq!(decode_frame(&[0, 0, 0, 10, 1, 2]), Err(HostError::Frame));
    assert_eq!(decode_frame(&[0, 0, 0]), Err(HostError::Frame));
    assert_eq!(decode_frame(&[]), Err(HostError::Frame));
    let mut short = encode_frame(b"abcdef")?;
    short.pop();
    assert_eq!(decode_frame(&short), Err(HostError::Frame));
    Ok(())
}
