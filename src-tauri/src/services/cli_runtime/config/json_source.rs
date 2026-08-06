use serde_json::Value as JsonValue;

pub(super) fn rewrite_claude_config(
    settings: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let parsed = serde_json::from_str::<JsonValue>(settings)
        .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
    let root = parsed
        .as_object()
        .ok_or_else(|| "Claude Code 配置文件格式无效".to_string())?;
    let source = JsonSourceParser::new(settings).parse_root_object()?;
    let env_value = root.get("env");
    let env = env_value.and_then(JsonValue::as_object);
    if env_value.is_some() && env.is_none() {
        return Err("Claude Code 配置中的 env 不是对象".to_string());
    }

    let env_source = source
        .members
        .iter()
        .rev()
        .find(|member| member.key == "env")
        .and_then(|member| member.object.as_ref());
    if env_value.is_some() && env_source.is_none() {
        return Err("Claude Code 配置中的 env 不是对象".to_string());
    }

    let base_url = base_url.trim();
    let api_key = api_key.trim();
    let has_auth_token = env.is_some_and(|env| env.contains_key("ANTHROPIC_AUTH_TOKEN"));
    let has_api_key = env.is_some_and(|env| env.contains_key("ANTHROPIC_API_KEY"));
    let mut fields = vec![("ANTHROPIC_BASE_URL", base_url)];
    if has_auth_token || !has_api_key {
        fields.push(("ANTHROPIC_AUTH_TOKEN", api_key));
    }
    if has_api_key {
        fields.push(("ANTHROPIC_API_KEY", api_key));
    }

    let mut edits = Vec::new();
    if let Some(env_source) = env_source {
        let mut missing = Vec::new();
        for (key, value) in fields {
            if let Some(member) = env_source
                .members
                .iter()
                .rev()
                .find(|member| member.key == key)
            {
                if !json_string_matches(settings, member.value_start, member.value_end, value) {
                    edits.push(JsonTextEdit {
                        start: member.value_start,
                        end: member.value_end,
                        replacement: json_string(value),
                    });
                }
            } else {
                missing.push((key, value));
            }
        }
        if !missing.is_empty() {
            edits.push(insert_missing_fields(settings, env_source, &missing));
        }
    } else {
        edits.push(insert_missing_env(settings, &source, &fields));
    }

    apply_json_edits(settings, edits)
}

#[derive(Debug)]
struct JsonObjectSpan {
    open: usize,
    close: usize,
    members: Vec<JsonMemberSpan>,
}

#[derive(Debug)]
struct JsonMemberSpan {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
    object: Option<JsonObjectSpan>,
}

struct JsonSourceParser<'a> {
    source: &'a str,
    bytes: &'a [u8],
    position: usize,
}

#[derive(Debug)]
struct JsonTextEdit {
    start: usize,
    end: usize,
    replacement: String,
}

impl<'a> JsonSourceParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            position: 0,
        }
    }

    fn parse_root_object(mut self) -> Result<JsonObjectSpan, String> {
        self.skip_whitespace();
        let object = self.parse_object()?;
        self.skip_whitespace();
        if self.position != self.bytes.len() {
            return Err("Claude Code 配置文件格式无效".to_string());
        }
        Ok(object)
    }

    fn parse_object(&mut self) -> Result<JsonObjectSpan, String> {
        let open = self.expect(b'{')?;
        let mut members = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            let close = self.expect(b'}')?;
            return Ok(JsonObjectSpan {
                open,
                close,
                members,
            });
        }

        loop {
            self.skip_whitespace();
            let key_start = self.position;
            let key_end = self.parse_string_end()?;
            let key = serde_json::from_str::<String>(&self.source[key_start..key_end])
                .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value_start = self.position;
            let object = self.parse_value()?;
            let value_end = self.position;
            members.push(JsonMemberSpan {
                key,
                key_start,
                value_start,
                value_end,
                object,
            });
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b'}') => {
                    let close = self.expect(b'}')?;
                    return Ok(JsonObjectSpan {
                        open,
                        close,
                        members,
                    });
                }
                _ => return Err("Claude Code 配置文件格式无效".to_string()),
            }
        }
    }

    fn parse_value(&mut self) -> Result<Option<JsonObjectSpan>, String> {
        match self.peek() {
            Some(b'{') => self.parse_object().map(Some),
            Some(b'[') => {
                self.parse_array()?;
                Ok(None)
            }
            Some(b'"') => {
                self.parse_string_end()?;
                Ok(None)
            }
            Some(b't') => {
                self.expect_literal(b"true")?;
                Ok(None)
            }
            Some(b'f') => {
                self.expect_literal(b"false")?;
                Ok(None)
            }
            Some(b'n') => {
                self.expect_literal(b"null")?;
                Ok(None)
            }
            Some(b'-' | b'0'..=b'9') => {
                self.parse_number();
                Ok(None)
            }
            _ => Err("Claude Code 配置文件格式无效".to_string()),
        }
    }

    fn parse_array(&mut self) -> Result<(), String> {
        self.expect(b'[')?;
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(());
        }
        loop {
            self.skip_whitespace();
            self.parse_value()?;
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(());
                }
                _ => return Err("Claude Code 配置文件格式无效".to_string()),
            }
        }
    }

    fn parse_string_end(&mut self) -> Result<usize, String> {
        self.expect(b'"')?;
        while let Some(byte) = self.peek() {
            self.position += 1;
            match byte {
                b'"' => return Ok(self.position),
                b'\\' => {
                    if self.peek().is_none() {
                        return Err("Claude Code 配置文件格式无效".to_string());
                    }
                    self.position += 1;
                }
                0..=0x1f => return Err("Claude Code 配置文件格式无效".to_string()),
                _ => {}
            }
        }
        Err("Claude Code 配置文件格式无效".to_string())
    }

    fn parse_number(&mut self) {
        while let Some(byte) = self.peek() {
            if matches!(byte, b' ' | b'\n' | b'\r' | b'\t' | b',' | b']' | b'}') {
                break;
            }
            self.position += 1;
        }
    }

    fn expect_literal(&mut self, literal: &[u8]) -> Result<(), String> {
        let end = self.position.saturating_add(literal.len());
        if self.bytes.get(self.position..end) != Some(literal) {
            return Err("Claude Code 配置文件格式无效".to_string());
        }
        self.position = end;
        Ok(())
    }

    fn expect(&mut self, expected: u8) -> Result<usize, String> {
        if self.peek() == Some(expected) {
            let position = self.position;
            self.position += 1;
            Ok(position)
        } else {
            Err("Claude Code 配置文件格式无效".to_string())
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

fn json_string_matches(source: &str, start: usize, end: usize, expected: &str) -> bool {
    serde_json::from_str::<String>(&source[start..end])
        .map(|value| value == expected)
        .unwrap_or(false)
}

fn insert_missing_fields(
    source: &str,
    object: &JsonObjectSpan,
    fields: &[(&str, &str)],
) -> JsonTextEdit {
    if object.members.is_empty() {
        return JsonTextEdit {
            start: object.open + 1,
            end: object.close,
            replacement: format_empty_object_contents(source, object, fields),
        };
    }

    let multiline = is_multiline(source, object);
    let replacement = if multiline {
        let indent = member_indent(source, object);
        format!(
            ",{}{}",
            newline_for(source),
            format_members_multiline(fields, &indent, newline_for(source))
        )
    } else {
        format!(", {}", format_members_inline(fields))
    };
    let last_value_end = object
        .members
        .last()
        .map(|member| member.value_end)
        .unwrap_or(object.open + 1);
    JsonTextEdit {
        start: last_value_end,
        end: last_value_end,
        replacement,
    }
}

fn insert_missing_env(
    source: &str,
    root: &JsonObjectSpan,
    fields: &[(&str, &str)],
) -> JsonTextEdit {
    let multiline = is_multiline(source, root);
    if root.members.is_empty() {
        return JsonTextEdit {
            start: root.open + 1,
            end: root.close,
            replacement: format_empty_root_contents(source, root, fields),
        };
    }

    let member_indent = member_indent(source, root);
    let replacement = if multiline {
        let child_indent = format!("{member_indent}{}", indentation_unit(source));
        let env = format!(
            "{}: {{{}{}{}{}{}",
            json_string("env"),
            newline_for(source),
            format_members_multiline(fields, &child_indent, newline_for(source)),
            newline_for(source),
            member_indent,
            "}"
        );
        format!(",{}{}{}", newline_for(source), member_indent, env)
    } else {
        format!(
            ", {}: {{{}}}",
            json_string("env"),
            format_members_inline(fields)
        )
    };
    let last_value_end = root
        .members
        .last()
        .map(|member| member.value_end)
        .unwrap_or(root.open + 1);
    JsonTextEdit {
        start: last_value_end,
        end: last_value_end,
        replacement,
    }
}

fn format_empty_object_contents(
    source: &str,
    object: &JsonObjectSpan,
    fields: &[(&str, &str)],
) -> String {
    if is_multiline(source, object) {
        let newline = newline_for(source);
        let closing_indent = line_indent_at(source, object.close);
        let indent = format!("{closing_indent}{}", indentation_unit(source));
        format!(
            "{}{}{}{}",
            newline,
            format_members_multiline(fields, &indent, newline),
            newline,
            closing_indent
        )
    } else if object.close > object.open + 1 {
        format!(" {} ", format_members_inline(fields))
    } else {
        format_members_inline(fields)
    }
}

fn format_empty_root_contents(
    source: &str,
    root: &JsonObjectSpan,
    fields: &[(&str, &str)],
) -> String {
    if is_multiline(source, root) {
        let newline = newline_for(source);
        let closing_indent = line_indent_at(source, root.close);
        let member_indent = format!("{closing_indent}{}", indentation_unit(source));
        let child_indent = format!("{member_indent}{}", indentation_unit(source));
        let nested = format!(
            "{}{}{}{}{}",
            newline,
            format_members_multiline(fields, &child_indent, newline),
            newline,
            member_indent,
            "}"
        );
        format!(
            "{}{}{}{}{}{}{}",
            newline,
            member_indent,
            json_string("env"),
            ": {",
            nested,
            newline,
            closing_indent
        )
    } else if root.close > root.open + 1 {
        format!(
            " {}: {{{}}} ",
            json_string("env"),
            format_members_inline(fields)
        )
    } else {
        format!(
            "{}: {{{}}}",
            json_string("env"),
            format_members_inline(fields)
        )
    }
}

fn format_members_inline(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{}: {}", json_string(key), json_string(value)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_members_multiline(fields: &[(&str, &str)], indent: &str, newline: &str) -> String {
    fields
        .iter()
        .enumerate()
        .map(|(index, (key, value))| {
            let comma = if index + 1 < fields.len() { "," } else { "" };
            format!(
                "{}{}: {}{}",
                indent,
                json_string(key),
                json_string(value),
                comma
            )
        })
        .collect::<Vec<_>>()
        .join(newline)
}

fn apply_json_edits(source: &str, mut edits: Vec<JsonTextEdit>) -> Result<String, String> {
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    let mut output = source.to_string();
    for edit in edits {
        if edit.start > edit.end || edit.end > output.len() {
            return Err("生成 Claude Code 配置失败: JSON 字段位置无效".to_string());
        }
        output.replace_range(edit.start..edit.end, &edit.replacement);
    }
    Ok(output)
}

fn is_multiline(source: &str, object: &JsonObjectSpan) -> bool {
    source[object.open..object.close].contains('\n')
}

fn newline_for(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn member_indent(source: &str, object: &JsonObjectSpan) -> String {
    object
        .members
        .first()
        .map(|member| line_indent_at(source, member.key_start))
        .unwrap_or_else(|| {
            format!(
                "{}{}",
                line_indent_at(source, object.open),
                indentation_unit(source)
            )
        })
}

fn indentation_unit(source: &str) -> String {
    source
        .lines()
        .filter_map(|line| {
            let prefix = line
                .chars()
                .take_while(|character| matches!(character, ' ' | '\t'))
                .collect::<String>();
            (!prefix.is_empty()
                && line[prefix.len()..]
                    .chars()
                    .any(|character| character != ' ' && character != '\t'))
            .then_some(prefix)
        })
        .min_by_key(|prefix| prefix.chars().count())
        .unwrap_or_else(|| "  ".to_string())
}

fn line_indent_at(source: &str, position: usize) -> String {
    let line_start = source[..position]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    source[line_start..position]
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .collect()
}
