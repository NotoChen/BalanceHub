use serde_json::Value as JsonValue;

pub(crate) fn rewrite_json_string_fields(
    source_text: &str,
    object_path: &[&str],
    fields: &[(&str, &str)],
) -> Result<String, String> {
    if object_path.is_empty() {
        return Err("JSON 字段路径不能为空".to_string());
    }
    let parsed = serde_json::from_str::<JsonValue>(source_text)
        .map_err(|_| "JSON 配置格式无效".to_string())?;
    let mut parsed_object = parsed
        .as_object()
        .ok_or_else(|| "JSON 配置根节点不是对象".to_string())?;
    let source = JsonSourceParser::new(source_text).parse_root_object()?;

    let mut parsed_depth = 0;
    for key in object_path {
        match parsed_object.get(*key) {
            Some(value) => {
                parsed_object = value.as_object().ok_or_else(|| {
                    format!(
                        "JSON 配置中的 {} 不是对象",
                        object_path[..=parsed_depth].join(".")
                    )
                })?;
                parsed_depth += 1;
            }
            None => break,
        }
    }

    let mut source_object = &source;
    let mut source_depth = 0;
    for key in object_path {
        let Some(next) = find_object(source_object, key) else {
            break;
        };
        source_object = next;
        source_depth += 1;
    }
    if source_depth != parsed_depth {
        return Err("JSON 配置字段位置无效".to_string());
    }

    let mut edits = Vec::new();
    if source_depth == object_path.len() {
        let mut missing = Vec::new();
        for &(key, value) in fields {
            if let Some(member) = source_object
                .members
                .iter()
                .rev()
                .find(|member| member.key == key)
            {
                if !json_string_matches(source_text, member.value_start, member.value_end, value) {
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
            edits.push(insert_missing_fields(source_text, source_object, &missing));
        }
    } else {
        edits.push(insert_nested_object_member(
            source_text,
            source_object,
            &object_path[source_depth..],
            fields,
        ));
    }

    apply_json_edits(source_text, edits)
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
            return Err("JSON 配置格式无效".to_string());
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
                .map_err(|_| "JSON 配置格式无效".to_string())?;
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
                _ => return Err("JSON 配置格式无效".to_string()),
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
            _ => Err("JSON 配置格式无效".to_string()),
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
                _ => return Err("JSON 配置格式无效".to_string()),
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
                        return Err("JSON 配置格式无效".to_string());
                    }
                    self.position += 1;
                }
                0..=0x1f => return Err("JSON 配置格式无效".to_string()),
                _ => {}
            }
        }
        Err("JSON 配置格式无效".to_string())
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
            return Err("JSON 配置格式无效".to_string());
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
            Err("JSON 配置格式无效".to_string())
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

fn find_object<'a>(object: &'a JsonObjectSpan, key: &str) -> Option<&'a JsonObjectSpan> {
    object
        .members
        .iter()
        .rev()
        .find(|member| member.key == key)
        .and_then(|member| member.object.as_ref())
}

fn insert_nested_object_member(
    source: &str,
    parent: &JsonObjectSpan,
    object_path: &[&str],
    fields: &[(&str, &str)],
) -> JsonTextEdit {
    debug_assert!(!object_path.is_empty());
    let multiline = is_multiline(source, parent);
    let member_indent = member_indent(source, parent);
    let member = if multiline {
        let newline = newline_for(source);
        format_nested_member_multiline(
            object_path,
            fields,
            &member_indent,
            &indentation_unit(source),
            newline,
        )
    } else {
        format_nested_member_inline(object_path, fields)
    };
    insert_raw_member(source, parent, &member, &member_indent)
}

fn format_nested_member_inline(object_path: &[&str], fields: &[(&str, &str)]) -> String {
    let key = json_string(object_path[0]);
    if object_path.len() == 1 {
        format!("{key}: {{{}}}", format_members_inline(fields))
    } else {
        format!(
            "{key}: {{{}}}",
            format_nested_member_inline(&object_path[1..], fields)
        )
    }
}

fn format_nested_member_multiline(
    object_path: &[&str],
    fields: &[(&str, &str)],
    current_indent: &str,
    indentation_unit: &str,
    newline: &str,
) -> String {
    let key = json_string(object_path[0]);
    let child_indent = format!("{current_indent}{indentation_unit}");
    let contents = if object_path.len() == 1 {
        format_members_multiline(fields, &child_indent, newline)
    } else {
        format!(
            "{child_indent}{}",
            format_nested_member_multiline(
                &object_path[1..],
                fields,
                &child_indent,
                indentation_unit,
                newline,
            )
        )
    };
    format!("{key}: {{{newline}{contents}{newline}{current_indent}}}")
}

fn insert_raw_member(
    source: &str,
    parent: &JsonObjectSpan,
    member: &str,
    member_indent: &str,
) -> JsonTextEdit {
    if parent.members.is_empty() {
        let replacement = if is_multiline(source, parent) {
            let newline = newline_for(source);
            let closing_indent = line_indent_at(source, parent.close);
            format!("{newline}{member_indent}{member}{newline}{closing_indent}")
        } else if parent.close > parent.open + 1 {
            format!(" {member} ")
        } else {
            member.to_string()
        };
        return JsonTextEdit {
            start: parent.open + 1,
            end: parent.close,
            replacement,
        };
    }

    let replacement = if is_multiline(source, parent) {
        format!(",{}{}{}", newline_for(source), member_indent, member)
    } else {
        format!(", {member}")
    };
    let last_value_end = parent
        .members
        .last()
        .map(|member| member.value_end)
        .unwrap_or(parent.open + 1);
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
            return Err("生成 JSON 配置失败: 字段位置无效".to_string());
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
