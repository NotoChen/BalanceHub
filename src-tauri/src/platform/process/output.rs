use std::collections::VecDeque;

#[derive(Clone, Copy, Default)]
pub(crate) enum OutputEncoding {
    #[default]
    Utf8,
    // CMD /U emits UTF-16LE even when its output is redirected to a pipe.
    #[cfg(any(windows, test))]
    Utf16Le,
}

pub(super) struct OutputBuffer {
    bytes: VecDeque<u8>,
    max_bytes: usize,
    encoding: OutputEncoding,
    dropped_odd: bool,
    pub(super) truncated: bool,
}

impl OutputBuffer {
    pub(super) fn new(max_bytes: usize, encoding: OutputEncoding) -> Self {
        Self {
            bytes: VecDeque::with_capacity(max_bytes.min(64 * 1024)),
            max_bytes: max_bytes.max(1),
            encoding,
            dropped_odd: false,
            truncated: false,
        }
    }

    pub(super) fn push(&mut self, chunk: &[u8]) {
        let excess = self
            .bytes
            .len()
            .saturating_add(chunk.len())
            .saturating_sub(self.max_bytes);
        let buffered = excess.min(self.bytes.len());
        self.bytes.drain(..buffered);
        self.bytes.extend(&chunk[excess - buffered..]);
        if excess > 0 {
            self.truncated = true;
            self.dropped_odd ^= excess % 2 == 1;
        }
    }

    pub(super) fn text(&self, finished: bool) -> String {
        let bytes = self.bytes.iter().copied().collect::<Vec<_>>();
        match self.encoding {
            OutputEncoding::Utf8 => {
                let start = if self.truncated {
                    bytes
                        .iter()
                        .take_while(|byte| **byte & 0xc0 == 0x80)
                        .count()
                } else {
                    0
                };
                let mut tail = &bytes[start..];
                if !finished {
                    if let Err(error) = std::str::from_utf8(tail) {
                        if error.error_len().is_none() {
                            tail = &tail[..error.valid_up_to()];
                        }
                    }
                }
                String::from_utf8_lossy(tail).into_owned()
            }
            #[cfg(any(windows, test))]
            OutputEncoding::Utf16Le => {
                // The bounded tail may begin halfway through a code unit or a
                // surrogate pair. Never reinterpret those bytes as a new unit.
                let start = usize::from(self.dropped_odd).min(bytes.len());
                let (pairs, remainder) = bytes[start..].as_chunks::<2>();
                let mut units = pairs
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect::<Vec<_>>();
                let skip_leading_unit = units.first().is_some_and(|unit| {
                    if self.truncated {
                        (0xdc00..=0xdfff).contains(unit)
                    } else {
                        *unit == 0xfeff
                    }
                });
                if skip_leading_unit {
                    units.remove(0);
                }
                if !finished
                    && units
                        .last()
                        .is_some_and(|unit| (0xd800..=0xdbff).contains(unit))
                {
                    units.pop();
                }
                let mut text = String::from_utf16_lossy(&units);
                if finished && !remainder.is_empty() {
                    text.push(char::REPLACEMENT_CHARACTER);
                }
                text
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn utf16_paths_survive_every_pipe_chunk_boundary() {
        let expected = "Path=C:\\用户\\项目 🦀\\bin\r\n";
        let bytes = utf16(expected);
        for split in 0..=bytes.len() {
            let mut buffer = OutputBuffer::new(4096, OutputEncoding::Utf16Le);
            buffer.push(&bytes[..split]);
            assert!(!buffer.text(false).contains(char::REPLACEMENT_CHARACTER));
            buffer.push(&bytes[split..]);
            assert_eq!(buffer.text(true), expected);
            assert!(!buffer.truncated);
        }
    }

    #[test]
    fn utf16_bounded_tail_keeps_code_units_and_surrogates_aligned() {
        let bytes = utf16("中文🦀末尾");
        for limit in 1..bytes.len() {
            for chunk_size in 1..=bytes.len() {
                let mut buffer = OutputBuffer::new(limit, OutputEncoding::Utf16Le);
                for chunk in bytes.chunks(chunk_size) {
                    buffer.push(chunk);
                }
                let text = buffer.text(true);
                assert!(
                    "中文🦀末尾".ends_with(&text),
                    "limit={limit}, chunk={chunk_size}, text={text}"
                );
                assert!(!text.contains(char::REPLACEMENT_CHARACTER));
                assert!(buffer.truncated);
                assert!(buffer.bytes.len() <= limit);
            }
        }
    }

    #[test]
    fn utf8_live_and_truncated_output_keep_character_boundaries() {
        let bytes = "开始 🦀 结束".as_bytes();
        for limit in 1..=bytes.len() {
            let mut buffer = OutputBuffer::new(limit, OutputEncoding::Utf8);
            for byte in bytes {
                buffer.push(&[*byte]);
                assert!(!buffer.text(false).contains(char::REPLACEMENT_CHARACTER));
            }
            assert!("开始 🦀 结束".ends_with(&buffer.text(true)));
        }
    }

    #[test]
    fn utf16_bom_and_incomplete_final_output_are_handled_explicitly() {
        let mut buffer = OutputBuffer::new(64, OutputEncoding::Utf16Le);
        buffer.push(&[0xff, 0xfe]);
        buffer.push(&utf16("中文"));
        assert_eq!(buffer.text(true), "中文");
        buffer.push(&[0x41]);
        assert_eq!(buffer.text(false), "中文");
        assert_eq!(buffer.text(true), "中文�");
    }
}
