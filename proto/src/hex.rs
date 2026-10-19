//! Lowercase hexadecimal, allocation-free on the encode path.

use alloc::string::String;
use alloc::vec::Vec;

const DIGITS: &[u8; 16] = b"0123456789abcdef";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

/// Decodes exactly `out.len()` bytes; trailing or truncated input is rejected.
pub fn decode_into(input: &[u8], out: &mut [u8]) -> Option<()> {
    if input.len() != out.len() * 2 {
        return None;
    }
    for (i, chunk) in input.chunks_exact(2).enumerate() {
        out[i] = (nibble(chunk[0])? << 4) | nibble(chunk[1])?;
    }
    Some(())
}

pub fn decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = alloc::vec![0u8; input.len() / 2];
    decode_into(input, &mut out)?;
    Some(out)
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let bytes = [0x00, 0x0f, 0xa5, 0xff];
        let text = encode(&bytes);
        assert_eq!(text, "000fa5ff");
        assert_eq!(decode(text.as_bytes()).unwrap(), bytes);
        assert!(decode(b"0").is_none());
        assert!(decode(b"zz").is_none());
    }
}
