use crate::models::{AccountQuotaWindow, AccountSubscription};

enum Field<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Float(f32),
    Fixed,
}

fn varint(bytes: &[u8], offset: &mut usize) -> Result<u64, String> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*offset).ok_or("Grok 额度数据不完整")?;
        *offset += 1;
        if shift == 63 && byte > 1 {
            return Err("Grok 额度整数溢出".into());
        }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    Err("Grok 额度整数无效".into())
}

fn fields(bytes: &[u8]) -> Result<Vec<(u64, Field<'_>)>, String> {
    let mut result = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        if result.len() >= 4096 {
            return Err("Grok 额度字段过多".into());
        }
        let tag = varint(bytes, &mut offset)?;
        let number = tag >> 3;
        if number == 0 {
            return Err("Grok 额度字段无效".into());
        }
        let value = match tag & 7 {
            0 => Field::Varint(varint(bytes, &mut offset)?),
            1 => {
                offset = offset
                    .checked_add(8)
                    .filter(|end| *end <= bytes.len())
                    .ok_or("Grok 额度数据不完整")?;
                Field::Fixed
            }
            2 => {
                let len = usize::try_from(varint(bytes, &mut offset)?)
                    .map_err(|_| "Grok 额度长度无效")?;
                let end = offset
                    .checked_add(len)
                    .filter(|end| *end <= bytes.len())
                    .ok_or("Grok 额度数据不完整")?;
                let slice = &bytes[offset..end];
                offset = end;
                Field::Bytes(slice)
            }
            5 => {
                let end = offset
                    .checked_add(4)
                    .filter(|end| *end <= bytes.len())
                    .ok_or("Grok 额度数据不完整")?;
                let value = f32::from_le_bytes(
                    bytes[offset..end]
                        .try_into()
                        .map_err(|_| "Grok 额度格式无效")?,
                );
                offset = end;
                Field::Float(value)
            }
            _ => return Err("Grok 额度字段类型暂不支持".into()),
        };
        result.push((number, value));
    }
    Ok(result)
}

pub(super) fn parse(bytes: &[u8]) -> Result<AccountSubscription, String> {
    let mut payloads = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let header = bytes.get(offset..offset + 5).ok_or("Grok 额度帧不完整")?;
        let length =
            u32::from_be_bytes(header[1..5].try_into().map_err(|_| "Grok 额度帧无效")?) as usize;
        offset += 5;
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or("Grok 额度帧不完整")?;
        let data = &bytes[offset..end];
        offset = end;
        match header[0] {
            0 => payloads.push(data),
            128 => {
                for line in String::from_utf8_lossy(data).lines() {
                    if let Some((name, value)) = line.split_once(':') {
                        if name.trim().eq_ignore_ascii_case("grpc-status") && value.trim() != "0" {
                            let code = value
                                .trim()
                                .parse::<u32>()
                                .map_err(|_| "Grok 额度返回无效状态")?;
                            return Err(if code == 16 {
                                "Grok 授权已失效，请重新授权".into()
                            } else {
                                format!("Grok 额度暂不可读取（gRPC {code}）")
                            });
                        }
                    }
                }
            }
            _ => return Err("Grok 额度压缩格式暂不支持".into()),
        }
    }
    for payload in payloads {
        let outer = fields(payload)?;
        for (number, field) in outer {
            let (1, Field::Bytes(credits)) = (number, field) else {
                continue;
            };
            let credits = fields(credits)?;
            let percent = credits.iter().find_map(|(n, field)| match (n, field) {
                (1, Field::Float(v)) if v.is_finite() && (0.0..=100.0).contains(v) => {
                    Some(f64::from(*v))
                }
                _ => None,
            });
            let reset = credits.iter().find_map(|(n, field)| match (n, field) {
                (5, Field::Bytes(timestamp)) => {
                    fields(timestamp)
                        .ok()?
                        .into_iter()
                        .find_map(|(n, f)| match (n, f) {
                            (1, Field::Varint(v)) => i64::try_from(v).ok().filter(|n| *n > 0),
                            _ => None,
                        })
                }
                _ => None,
            });
            let has_period = credits
                .iter()
                .any(|(n, f)| matches!((n, f), (6, Field::Bytes(_)) | (8, Field::Bytes(_))));
            let percent = percent.or_else(|| {
                (reset.is_some()
                    && has_period
                    && !credits.iter().any(|(_, f)| matches!(f, Field::Float(_))))
                .then_some(0.0)
            });
            if let Some(percent) = percent {
                return Ok(AccountSubscription {
                    windows: vec![AccountQuotaWindow {
                        id: "grok-period".into(),
                        label: "当前周期".into(),
                        used_percent: Some(percent),
                        reset_at: reset,
                        ..Default::default()
                    }],
                    ..Default::default()
                });
            }
        }
    }
    Err("Grok 额度格式发生变化，未将未知字段当作额度".into())
}
