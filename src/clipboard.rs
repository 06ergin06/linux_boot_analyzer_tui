//! Clipboard through OSC 52: no `xclip`/`wl-copy`, and it travels over SSH.
//! tmux only forwards the sequence with `set -g set-clipboard on`.

use std::io::Write;

pub fn copy(text: &str) {
    let encoded = encode_base64(text.as_bytes());
    let mut stdout = std::io::stdout();
    let _ = write!(stdout, "\x1b]52;c;{encoded}\x07");
    let _ = stdout.flush();
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(data: &[u8]) -> String {
    let mut encoded = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let first = chunk[0] as u32;
        let second = chunk.get(1).copied().unwrap_or(0) as u32;
        let third = chunk.get(2).copied().unwrap_or(0) as u32;
        let group = (first << 16) | (second << 8) | third;

        encoded.push(ALPHABET[(group >> 18 & 63) as usize] as char);
        encoded.push(ALPHABET[(group >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(ALPHABET[(group >> 6 & 63) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(ALPHABET[(group & 63) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_examples() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foob"), "Zm9vYg==");
        assert_eq!(encode_base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn utf8_text_is_encoded_byte_wise() {
        assert_eq!(encode_base64("ş".as_bytes()), "xZ8=");
    }
}
