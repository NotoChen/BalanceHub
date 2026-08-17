#[derive(Debug)]
struct EnvAssignment {
    key: String,
    value_start: usize,
    value_end: usize,
    value: String,
    quote: Option<u8>,
}

pub(crate) fn env_value(source: &str, key: &str) -> Option<String> {
    let mut value = None;
    for assignment in assignments(source) {
        if assignment.key == key {
            value = non_empty(assignment.value);
        }
    }
    value
}

pub(crate) fn rewrite_env_values(source: &str, fields: &[(&str, &str)]) -> Result<String, String> {
    for (_, value) in fields {
        validate_env_value(value.trim())?;
    }

    let parsed = assignments(source);
    let mut edits = Vec::new();
    let mut found = std::collections::BTreeSet::new();
    for assignment in parsed {
        let Some((key, value)) = fields.iter().find(|(key, _)| *key == assignment.key) else {
            continue;
        };
        found.insert(*key);
        let replacement = encode_env_value(value.trim(), assignment.quote)?;
        if source[assignment.value_start..assignment.value_end] != replacement {
            edits.push((assignment.value_start, assignment.value_end, replacement));
        }
    }

    edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut output = source.to_string();
    for (start, end, replacement) in edits {
        output.replace_range(start..end, &replacement);
    }

    let missing = fields
        .iter()
        .filter(|(key, _)| !found.contains(key))
        .map(|(key, value)| Ok((*key, encode_env_value(value.trim(), None)?)))
        .collect::<Result<Vec<_>, String>>()?;
    if missing.is_empty() {
        return Ok(output);
    }

    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    if !output.is_empty() && !output.ends_with('\n') && !output.ends_with('\r') {
        output.push_str(newline);
    }
    for (index, (key, value)) in missing.into_iter().enumerate() {
        if index > 0 {
            output.push_str(newline);
        }
        output.push_str(key);
        output.push('=');
        output.push_str(&value);
    }
    output.push_str(newline);
    Ok(output)
}

fn assignments(source: &str) -> Vec<EnvAssignment> {
    let mut output = Vec::new();
    let mut line_start = 0;
    for line in source.split_inclusive('\n') {
        let line_end = line_start + line.trim_end_matches('\n').len();
        if let Some(assignment) = parse_assignment(source, line_start, line_end) {
            output.push(assignment);
        }
        line_start += line.len();
    }
    if line_start < source.len() {
        if let Some(assignment) = parse_assignment(source, line_start, source.len()) {
            output.push(assignment);
        }
    }
    output
}

fn parse_assignment(source: &str, line_start: usize, line_end: usize) -> Option<EnvAssignment> {
    let bytes = source.as_bytes();
    let mut position = skip_space(bytes, line_start, line_end);
    if bytes.get(position) == Some(&b'#') || position >= line_end {
        return None;
    }

    if source[position..line_end].starts_with("export") {
        let export_end = position + "export".len();
        if export_end < line_end && is_space(bytes[export_end]) {
            position = skip_space(bytes, export_end, line_end);
        }
    }

    let key_start = position;
    while position < line_end && is_key_byte(bytes[position]) {
        position += 1;
    }
    if position == key_start {
        return None;
    }
    let key = source[key_start..position].to_string();
    position = skip_space(bytes, position, line_end);
    match bytes.get(position).copied() {
        Some(b'=') => position += 1,
        Some(b':') if bytes.get(position + 1).is_some_and(|byte| is_space(*byte)) => {
            position += 1;
        }
        _ => return None,
    }
    position = skip_space(bytes, position, line_end);
    let value_start = position;
    let (value_end, quote) = value_end(bytes, value_start, line_end);
    let value = decode_env_value(&source[value_start..value_end], quote);
    Some(EnvAssignment {
        key,
        value_start,
        value_end,
        value,
        quote,
    })
}

fn value_end(bytes: &[u8], start: usize, line_end: usize) -> (usize, Option<u8>) {
    let quote = bytes
        .get(start)
        .copied()
        .filter(|byte| matches!(byte, b'\'' | b'"' | b'`'));
    if let Some(quote) = quote {
        let mut position = start + 1;
        while position < line_end {
            if bytes[position] == b'\\' && position + 1 < line_end {
                position += 2;
                continue;
            }
            if bytes[position] == quote {
                return (position + 1, Some(quote));
            }
            position += 1;
        }
    }

    let mut end = start;
    while end < line_end && bytes[end] != b'#' {
        end += 1;
    }
    while end > start && is_space(bytes[end - 1]) {
        end -= 1;
    }
    (end, None)
}

fn decode_env_value(raw: &str, quote: Option<u8>) -> String {
    let value = if quote.is_some() && raw.len() >= 2 {
        &raw[1..raw.len() - 1]
    } else {
        raw.trim()
    };
    if quote == Some(b'"') {
        value.replace("\\n", "\n").replace("\\r", "\r")
    } else {
        value.to_string()
    }
}

fn encode_env_value(value: &str, preferred_quote: Option<u8>) -> Result<String, String> {
    if preferred_quote.is_none() && is_plain_env_value(value) {
        return Ok(value.to_string());
    }
    for quote in preferred_quote.into_iter().chain(b"'\"`".iter().copied()) {
        if !value.as_bytes().contains(&quote) {
            return Ok(format!("{}{}{}", quote as char, value, quote as char));
        }
    }
    Err("配置值同时包含单引号、双引号和反引号，无法安全写入 .env".to_string())
}

fn validate_env_value(value: &str) -> Result<(), String> {
    if value
        .chars()
        .any(|character| matches!(character, '\0' | '\n' | '\r'))
    {
        return Err("配置值不能包含换行或空字符".to_string());
    }
    Ok(())
}

fn is_plain_env_value(value: &str) -> bool {
    !value.is_empty()
        && !value.bytes().any(|byte| {
            is_space(byte) || matches!(byte, b'#' | b'\'' | b'"' | b'`' | b'\r' | b'\n')
        })
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn skip_space(bytes: &[u8], mut position: usize, end: usize) -> usize {
    while position < end && is_space(bytes[position]) {
        position += 1;
    }
    position
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r')
}

fn is_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
}
