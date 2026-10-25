use super::{MAX_BODY_BYTES, MAX_HEADER_BYTES, RequestFraming};

#[test]
fn fragmented_headers_complete_only_after_terminator() -> Result<(), String> {
    let mut framing = RequestFraming::default();
    if framing.push(b"GET /containers/id/json HTTP/1.1\r\n")? {
        return Err("The request completed before its headers arrived.".to_owned());
    }
    let mut long_header = b"X-Test: ".to_vec();
    long_header.extend(std::iter::repeat_n(b'x', 300));
    long_header.extend_from_slice(b"\r\n");
    if framing.push(&long_header)? {
        return Err("The request completed before the header terminator.".to_owned());
    }
    if framing.push(b"Connection: close\r\n\r")? {
        return Err("A partial header terminator completed the request.".to_owned());
    }
    if !framing.push(b"\n")? {
        return Err("The parser did not recognize the complete request with no body.".to_owned());
    }
    if framing.finish()? != "GET /containers/id/json HTTP/1.1" {
        return Err("The parser returned the wrong request line.".to_owned());
    }
    Ok(())
}

#[test]
fn fixed_length_body_handles_split_and_coalesced_bytes() -> Result<(), String> {
    let mut split = RequestFraming::default();
    if split.push(b"POST /test HTTP/1.1\r\nContent-Length: 5\r\n\r\nhe")? {
        return Err("A partial fixed-length body completed the request.".to_owned());
    }
    if !split.push(b"llo")? || split.finish()? != "POST /test HTTP/1.1" {
        return Err("The parser did not complete a request with a body supplied in separate byte sequences.".to_owned());
    }

    let mut coalesced = RequestFraming::default();
    let request = b"POST /test HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
    if !coalesced.push(request)? || coalesced.finish()? != "POST /test HTTP/1.1" {
        return Err(
            "The parser did not complete a request supplied in one byte sequence.".to_owned(),
        );
    }
    Ok(())
}

#[test]
fn incomplete_headers_and_bodies_fail_closed() -> Result<(), String> {
    let mut incomplete_header = RequestFraming::default();
    incomplete_header.push(b"GET / HTTP/1.1\r\nHost: local\r\n")?;
    if incomplete_header.finish().is_ok() {
        return Err("The parser accepted incomplete headers.".to_owned());
    }

    let mut incomplete_body = RequestFraming::default();
    incomplete_body.push(b"POST / HTTP/1.1\r\nContent-Length: 3\r\n\r\nx")?;
    if incomplete_body.finish().is_ok() {
        return Err("The parser accepted an incomplete body.".to_owned());
    }

    Ok(())
}

#[test]
fn conflicting_lengths_and_transfer_encoding_fail_closed() -> Result<(), String> {
    for head in [
        b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n".as_slice(),
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".as_slice(),
    ] {
        let mut framing = RequestFraming::default();
        if framing.push(head).is_ok() {
            return Err("The parser accepted an unsupported request header.".to_owned());
        }
    }
    Ok(())
}

#[test]
fn limits_invalid_lengths_and_extra_body_bytes_fail_closed() -> Result<(), String> {
    for value in ["", "+1", "x", "18446744073709551616"] {
        let head = format!("POST / HTTP/1.1\r\nContent-Length: {value}\r\n\r\n");
        let mut framing = RequestFraming::default();
        if framing.push(head.as_bytes()).is_ok() {
            return Err("The parser accepted an invalid Content-Length value.".to_owned());
        }
    }

    let mut oversized_header = b"GET / HTTP/1.1\r\nX-Test: ".to_vec();
    oversized_header.extend(std::iter::repeat_n(b'x', MAX_HEADER_BYTES));
    oversized_header.extend_from_slice(b"\r\n\r\n");
    let mut framing = RequestFraming::default();
    if framing.push(&oversized_header).is_ok() {
        return Err("The parser accepted headers above the configured byte limit.".to_owned());
    }

    let head = format!(
        "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        MAX_BODY_BYTES + 1
    );
    let mut framing = RequestFraming::default();
    if framing.push(head.as_bytes()).is_ok() {
        return Err("The parser accepted a body above the configured byte limit.".to_owned());
    }

    let mut framing = RequestFraming::default();
    if framing
        .push(b"POST / HTTP/1.1\r\nContent-Length: 1\r\n\r\nxy")
        .is_ok()
    {
        return Err("The parser accepted bytes beyond the declared body length.".to_owned());
    }
    Ok(())
}
