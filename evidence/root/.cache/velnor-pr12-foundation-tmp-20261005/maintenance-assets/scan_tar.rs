use std::io::{self, Read};

fn main() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut index = 0_u64;
    loop {
        let mut header = [0_u8; 512];
        match input.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        }
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let size_text = header[124..136]
            .iter()
            .copied()
            .take_while(|byte| *byte != 0)
            .filter(|byte| *byte != b' ')
            .collect::<Vec<_>>();
        let size = std::str::from_utf8(&size_text)
            .ok()
            .and_then(|value| u64::from_str_radix(value, 8).ok())
            .unwrap_or(0);
        let kind = header[156];
        if matches!(kind, b'x' | b'g' | b'L' | b'K') {
            let name = String::from_utf8_lossy(&header[..100]);
            println!("entry={index} kind={} size={size} name={name:?}", char::from(kind));
        }
        let padded = size.saturating_add(511) / 512 * 512;
        io::copy(&mut input.by_ref().take(padded), &mut io::sink())?;
        index += 1;
    }
    Ok(())
}
