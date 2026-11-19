// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, BufRead, Read};

use flate2::bufread::GzDecoder;

#[derive(Debug)]
enum State<R: BufRead> {
    Between(R),
    Inside(Box<GzDecoder<R>>),
    Finished,
}

#[derive(Debug)]
pub struct GzipMembers<R: BufRead> {
    state: State<R>,
}

impl<R: BufRead> GzipMembers<R> {
    pub fn new(inner: R) -> Self {
        Self {
            state: State::Between(inner),
        }
    }
}

fn skip_zeros<R: BufRead>(reader: &mut R) -> io::Result<bool> {
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(false);
        }
        let zeros = buffer.iter().take_while(|byte| **byte == 0).count();
        if zeros < buffer.len() {
            reader.consume(zeros);
            return Ok(true);
        }
        reader.consume(zeros);
    }
}

impl<R: BufRead> Read for GzipMembers<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match std::mem::replace(&mut self.state, State::Finished) {
                State::Between(mut reader) => {
                    if skip_zeros(&mut reader)? {
                        self.state = State::Inside(Box::new(GzDecoder::new(reader)));
                    } else {
                        return Ok(0);
                    }
                }
                State::Inside(mut decoder) => {
                    let count = decoder.read(buf)?;
                    if count == 0 {
                        self.state = State::Between(decoder.into_inner());
                    } else {
                        self.state = State::Inside(decoder);
                        return Ok(count);
                    }
                }
                State::Finished => return Ok(0),
            }
        }
    }
}
