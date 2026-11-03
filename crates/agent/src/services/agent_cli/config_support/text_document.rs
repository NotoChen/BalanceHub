//! Preserve native text envelopes across editors and structured patches.
pub(crate) fn body(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

pub(crate) fn decode(bytes: &[u8]) -> Result<&str, std::str::Utf8Error> {
    std::str::from_utf8(bytes).map(body)
}

/// Restore a BOM and a consistent CRLF convention from the original file.
/// Mixed endings stay as authored; this does not reformat the user's content.
pub(crate) fn restore_layout(original: &str, edited: &str) -> String {
    let edited = body(edited);
    let crlf = original.contains("\r\n")
        && !original
            .as_bytes()
            .windows(2)
            .any(|pair| pair[1] == b'\n' && pair[0] != b'\r')
        && !original.starts_with('\n');
    let text = if crlf {
        edited.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        edited.to_owned()
    };
    if original.starts_with('\u{feff}') {
        format!("\u{feff}{text}")
    } else {
        text
    }
}
