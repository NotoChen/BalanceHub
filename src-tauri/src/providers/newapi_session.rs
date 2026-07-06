use super::anyrouter::normalize_session_cookie;

pub fn decode_session_user_id(raw: &str) -> Option<String> {
    let session = normalize_session_cookie(raw);
    let outer = base64_url_decode(&session)?;
    let parts = split_bytes(&outer, b'|');
    if parts.len() != 3 {
        return None;
    }

    let inner_text = std::str::from_utf8(parts[1]).ok()?;
    let gob_data = base64_url_decode(inner_text)?;
    parse_gob_session_map(&gob_data)
        .into_iter()
        .find(|(key, _)| key == "id")
        .map(|(_, value)| value)
        .filter(|value| !value.trim().is_empty())
}

fn base64_url_decode(raw: &str) -> Option<Vec<u8>> {
    let mut bits: u32 = 0;
    let mut bit_count: u8 = 0;
    let mut output = Vec::new();

    for byte in raw.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b'\r' | b'\n' | b'\t' | b' ' => continue,
            _ => return None,
        } as u32;

        bits = (bits << 6) | value;
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            output.push(((bits >> bit_count) & 0xff) as u8);
        }
    }

    Some(output)
}

fn split_bytes(input: &[u8], delimiter: u8) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (index, byte) in input.iter().enumerate() {
        if *byte == delimiter {
            parts.push(&input[start..index]);
            start = index + 1;
        }
    }
    parts.push(&input[start..]);
    parts
}

fn parse_gob_session_map(buf: &[u8]) -> Vec<(String, String)> {
    let marker = [0x06, b's', b't', b'r', b'i', b'n', b'g'];
    let Some(first_string) = buf
        .windows(marker.len())
        .position(|window| window == marker)
    else {
        return Vec::new();
    };
    if first_string == 0 {
        return Vec::new();
    }

    let map_count = buf[first_string - 1] as usize;
    let mut pos = first_string;
    let mut result = Vec::new();

    for _ in 0..map_count {
        let Some(key_type_len) = read_u8(buf, &mut pos) else {
            break;
        };
        if !skip(buf, &mut pos, key_type_len as usize) {
            break;
        }
        if read_u8(buf, &mut pos).is_none() {
            break;
        }
        let Some(_key_content_len) = read_u8(buf, &mut pos) else {
            break;
        };
        if read_u8(buf, &mut pos).is_none() {
            break;
        }
        let Some(key_len) = read_u8(buf, &mut pos) else {
            break;
        };
        let Some(key_name) = read_string(buf, &mut pos, key_len as usize) else {
            break;
        };

        let Some(value_type_len) = read_u8(buf, &mut pos) else {
            break;
        };
        let Some(value_type) = read_string(buf, &mut pos, value_type_len as usize) else {
            break;
        };

        if value_type == "int" {
            if read_u8(buf, &mut pos).is_none() {
                break;
            }
            let Some(_value_content_len) = read_u8(buf, &mut pos) else {
                break;
            };
            if read_u8(buf, &mut pos).is_none() {
                break;
            }
            if let Some(value) = decode_gob_int(buf, &mut pos) {
                result.push((key_name, value.to_string()));
            }
        } else if value_type == "string" {
            if read_u8(buf, &mut pos).is_none() {
                break;
            }
            let Some(_value_content_len) = read_u8(buf, &mut pos) else {
                break;
            };
            if read_u8(buf, &mut pos).is_none() {
                break;
            }
            let Some(value_len) = read_u8(buf, &mut pos) else {
                break;
            };
            if let Some(value) = read_string(buf, &mut pos, value_len as usize) {
                result.push((key_name, value));
            }
        }
    }

    result
}

fn read_u8(buf: &[u8], pos: &mut usize) -> Option<u8> {
    let value = *buf.get(*pos)?;
    *pos += 1;
    Some(value)
}

fn skip(buf: &[u8], pos: &mut usize, len: usize) -> bool {
    if pos.saturating_add(len) > buf.len() {
        return false;
    }
    *pos += len;
    true
}

fn read_string(buf: &[u8], pos: &mut usize, len: usize) -> Option<String> {
    if pos.saturating_add(len) > buf.len() {
        return None;
    }
    let value = std::str::from_utf8(&buf[*pos..*pos + len])
        .ok()?
        .to_string();
    *pos += len;
    Some(value)
}

fn decode_gob_int(buf: &[u8], pos: &mut usize) -> Option<i64> {
    let first = read_u8(buf, pos)?;
    let unsigned = if first <= 0x7f {
        first as u64
    } else {
        let bytes = 256usize.saturating_sub(first as usize);
        let mut value = 0u64;
        for _ in 0..bytes {
            value = value
                .checked_mul(256)?
                .checked_add(read_u8(buf, pos)? as u64)?;
        }
        value
    };

    if unsigned % 2 == 0 {
        Some((unsigned / 2) as i64)
    } else {
        Some(-((unsigned as i64 + 1) / 2))
    }
}

#[cfg(test)]
mod tests {
    use super::decode_session_user_id;

    const SYNTHETIC_SESSION_COOKIE: &str =
        "MHxBUVp6ZEhKcGJtY0FBQUFDYVdRRGFXNTBBQUFBL21CeXxzeW50aGV0aWMtc2lnbmF0dXJl";

    #[test]
    fn decodes_anyrouter_secure_cookie_user_id() {
        assert_eq!(
            decode_session_user_id(SYNTHETIC_SESSION_COOKIE).as_deref(),
            Some("12345")
        );
    }
}
