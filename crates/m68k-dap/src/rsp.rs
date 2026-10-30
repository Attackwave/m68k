//! GDB Remote Serial Protocol framing.
//!
//! The UAE builds with a remote debugger (WinUAE `debugging_features=gdbserver`,
//! patched FS-UAE `--remote_debugger`) speak plain RSP: `$payload#checksum`,
//! with `}` escapes and `*` run-length encoding in replies.

use std::io::{self, Read, Write};

/// Frame a packet: `$payload#checksum`.
pub fn frame(payload: &str) -> String {
    let sum = payload.bytes().fold(0u8, |a, b| a.wrapping_add(b));
    format!("${payload}#{sum:02x}")
}

/// Incremental packet parser: feed it bytes, get complete payloads out.
/// Acks (`+`/`-`) and anything outside a packet are dropped.
#[derive(Default)]
pub struct Parser {
    buf: Vec<u8>,
}

impl Parser {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut packets = Vec::new();
        loop {
            let Some(start) = self.buf.iter().position(|&b| b == b'$') else {
                self.buf.clear();
                return packets;
            };
            // Payload may itself contain '#' only when escaped, so the first
            // '#' after '$' ends it; two checksum digits follow.
            let Some(hash) = self.buf[start..].iter().position(|&b| b == b'#') else {
                self.buf.drain(..start);
                return packets;
            };
            let end = start + hash + 3;
            if self.buf.len() < end {
                self.buf.drain(..start);
                return packets;
            }
            packets.push(decode(&self.buf[start + 1..start + hash]));
            self.buf.drain(..end);
        }
    }
}

/// Undo `}` escaping and `*` run-length encoding.
fn decode(raw: &[u8]) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'}' if i + 1 < raw.len() => {
                out.push(raw[i + 1] ^ 0x20);
                i += 2;
            }
            b'*' if i + 1 < raw.len() => {
                if let Some(&prev) = out.last() {
                    let count = raw[i + 1].saturating_sub(29) as usize;
                    out.extend(std::iter::repeat_n(prev, count));
                }
                i += 2;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Blocking reader loop: calls `on_packet` for every packet until EOF.
pub fn read_packets(mut stream: impl Read, mut on_packet: impl FnMut(String)) -> io::Result<()> {
    let mut parser = Parser::default();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        for p in parser.feed(&buf[..n]) {
            on_packet(p);
        }
    }
}

pub fn send(mut stream: impl Write, payload: &str) -> io::Result<()> {
    stream.write_all(frame(payload).as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_with_checksum() {
        assert_eq!(frame("qOffsets"), "$qOffsets#4b");
    }

    #[test]
    fn parses_split_packets_escapes_and_rle() {
        let mut p = Parser::default();
        assert!(p.feed(b"+$O").is_empty());
        assert_eq!(p.feed(b"K#9a$0* #00"), vec!["OK", "0000"]);
        assert_eq!(p.feed(b"$a}\x03b#00"), vec!["a#b"]);
    }
}
