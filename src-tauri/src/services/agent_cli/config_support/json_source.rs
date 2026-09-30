use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;

pub(crate) fn rewrite_json_string_fields(
    source_text: &str,
    object_path: &[&str],
    fields: &[(&str, &str)],
) -> Result<String, String> {
    // An empty object path selects the root object, for example Codex auth.
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
    commas: Vec<usize>,
}

#[derive(Debug)]
struct JsonMemberSpan {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
    object: Option<JsonObjectSpan>,
    array: Option<JsonArraySpan>,
}

#[derive(Debug)]
struct JsonArraySpan {
    open: usize,
    close: usize,
    members: Vec<JsonArrayMemberSpan>,
    commas: Vec<usize>,
}

#[derive(Debug)]
struct JsonArrayMemberSpan {
    start: usize,
    end: usize,
    object: Option<JsonObjectSpan>,
    array: Option<JsonArraySpan>,
}

struct JsonSourceParser<'a> {
    source: &'a str,
    bytes: &'a [u8],
    position: usize,
    comments: bool,
    depth: usize,
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
            comments: false,
            depth: 0,
        }
    }

    fn parse_root_object(mut self) -> Result<JsonObjectSpan, String> {
        self.skip_whitespace()?;
        let object = self.parse_object()?;
        self.skip_whitespace()?;
        if self.position != self.bytes.len() {
            return Err("JSON 配置格式无效".to_string());
        }
        Ok(object)
    }

    fn parse_object(&mut self) -> Result<JsonObjectSpan, String> {
        let open = self.expect(b'{')?;
        let mut members = Vec::new();
        let mut commas = Vec::new();
        let mut keys = BTreeSet::new();
        self.skip_whitespace()?;
        if self.peek() == Some(b'}') {
            let close = self.expect(b'}')?;
            return Ok(JsonObjectSpan {
                open,
                close,
                members,
                commas,
            });
        }

        loop {
            self.skip_whitespace()?;
            let key_start = self.position;
            let key_end = self.parse_string_end()?;
            let key = serde_json::from_str::<String>(&self.source[key_start..key_end])
                .map_err(|_| "JSON 配置格式无效".to_string())?;
            if !keys.insert(key.clone()) {
                return Err("JSON 配置包含重复字段".to_owned());
            }
            self.skip_whitespace()?;
            self.expect(b':')?;
            self.skip_whitespace()?;
            let value_start = self.position;
            let (object, array) = self.parse_value()?;
            let value_end = self.position;
            members.push(JsonMemberSpan {
                key,
                key_start,
                value_start,
                value_end,
                object,
                array,
            });
            self.skip_whitespace()?;
            match self.peek() {
                Some(b',') => {
                    commas.push(self.position);
                    self.position += 1;
                    self.skip_whitespace()?;
                }
                Some(b'}') => {
                    let close = self.expect(b'}')?;
                    return Ok(JsonObjectSpan {
                        open,
                        close,
                        members,
                        commas,
                    });
                }
                _ => return Err("JSON 配置格式无效".to_string()),
            }
        }
    }

    fn parse_value(&mut self) -> Result<(Option<JsonObjectSpan>, Option<JsonArraySpan>), String> {
        if self.depth >= 128 {
            return Err("JSON 配置嵌套过深".to_owned());
        }
        self.depth += 1;
        let start = self.position;
        let result = match self.peek() {
            Some(b'{') => self.parse_object().map(|object| (Some(object), None)),
            Some(b'[') => self.parse_array().map(|array| (None, Some(array))),
            Some(b'"') => {
                self.parse_string_end()?;
                serde_json::from_str::<String>(&self.source[start..self.position])
                    .map_err(|_| "JSON 配置格式无效".to_owned())?;
                Ok((None, None))
            }
            Some(b't') => {
                self.expect_literal(b"true")?;
                Ok((None, None))
            }
            Some(b'f') => {
                self.expect_literal(b"false")?;
                Ok((None, None))
            }
            Some(b'n') => {
                self.expect_literal(b"null")?;
                Ok((None, None))
            }
            Some(b'-' | b'0'..=b'9') => {
                self.parse_number();
                serde_json::from_str::<serde_json::Number>(&self.source[start..self.position])
                    .map_err(|_| "JSON 配置格式无效".to_owned())?;
                Ok((None, None))
            }
            _ => Err("JSON 配置格式无效".to_string()),
        };
        self.depth -= 1;
        result
    }

    fn parse_array(&mut self) -> Result<JsonArraySpan, String> {
        let open = self.expect(b'[')?;
        let mut members = Vec::new();
        let mut commas = Vec::new();
        self.skip_whitespace()?;
        if self.peek() == Some(b']') {
            let close = self.expect(b']')?;
            return Ok(JsonArraySpan {
                open,
                close,
                members,
                commas,
            });
        }
        loop {
            self.skip_whitespace()?;
            let start = self.position;
            let (object, array) = self.parse_value()?;
            members.push(JsonArrayMemberSpan {
                start,
                end: self.position,
                object,
                array,
            });
            self.skip_whitespace()?;
            match self.peek() {
                Some(b',') => {
                    commas.push(self.position);
                    self.position += 1;
                    self.skip_whitespace()?;
                }
                Some(b']') => {
                    let close = self.expect(b']')?;
                    return Ok(JsonArraySpan {
                        open,
                        close,
                        members,
                        commas,
                    });
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
            if matches!(
                byte,
                b' ' | b'\n' | b'\r' | b'\t' | b',' | b']' | b'}' | b'/'
            ) {
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

    fn skip_whitespace(&mut self) -> Result<(), String> {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
                self.position += 1;
            }
            if !self.comments || self.peek() != Some(b'/') {
                return Ok(());
            }
            match self.bytes.get(self.position + 1) {
                Some(b'/') => {
                    self.position += 2;
                    while self.peek().is_some_and(|byte| byte != b'\n') {
                        self.position += 1;
                    }
                }
                Some(b'*') => {
                    self.position += 2;
                    while self.position + 1 < self.bytes.len()
                        && self.bytes.get(self.position..self.position + 2) != Some(b"*/")
                    {
                        self.position += 1;
                    }
                    if self.position + 1 >= self.bytes.len() {
                        return Err("JSON 配置注释未结束".to_owned());
                    }
                    self.position += 2;
                }
                _ => return Err("JSON 配置格式无效".to_owned()),
            }
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

/// Gemini strips comments before strict JSON decoding, so trailing commas
/// remain invalid. Duplicate-key and depth checks share the strict scanner.
pub(crate) fn parse_jsonc_document(source: &str) -> Result<JsonValue, String> {
    let mut parser = JsonSourceParser::new(source);
    parser.comments = true;
    object_value(source, &parser.parse_root_object()?)
}

/// Locate an existing native value with the same strict scanner used for edits.
/// The path is resolved by a backend adapter, never accepted from an IPC caller.
pub(crate) fn json_value_range(
    source: &str,
    path: &[String],
    comments: bool,
) -> Result<std::ops::Range<usize>, String> {
    let mut parser = JsonSourceParser::new(source);
    parser.comments = comments;
    let root = parser.parse_root_object()?;
    let mut object = Some(&root);
    let mut array: Option<&JsonArraySpan> = None;
    let mut range = root.open..root.close + 1;
    for key in path {
        if let Some(current) = object {
            let member = current
                .members
                .iter()
                .find(|member| &member.key == key)
                .ok_or("配置条目已不存在")?;
            range = member.value_start..member.value_end;
            object = member.object.as_ref();
            array = member.array.as_ref();
        } else if let Some(current) = array {
            let index = key.parse::<usize>().map_err(|_| "配置条目位置无效")?;
            let member = current.members.get(index).ok_or("配置条目已不存在")?;
            range = member.start..member.end;
            object = member.object.as_ref();
            array = member.array.as_ref();
        } else {
            return Err("配置条目位置已变化".to_owned());
        }
    }
    Ok(range)
}

/// Rewrite only changed values. Unchanged values retain their exact text, and
/// container edits retain comments, unknown members, order and line endings.
/// The desired document comes from a native adapter, never from a client path.
pub(crate) fn rewrite_json_document(
    source: &str,
    desired: &JsonValue,
    comments: bool,
) -> Result<String, String> {
    let mut parser = JsonSourceParser::new(source);
    parser.comments = comments;
    let root = parser.parse_root_object()?;
    let desired = desired.as_object().ok_or("JSON 配置根节点不是对象")?;
    let replacement = render_object(source, &root, desired)?;
    let output = format!(
        "{}{}{}",
        &source[..root.open],
        replacement,
        &source[root.close + 1..]
    );
    let mut verified = JsonSourceParser::new(&output);
    verified.comments = comments;
    if object_value(&output, &verified.parse_root_object()?)? != JsonValue::Object(desired.clone())
    {
        return Err("JSON 配置修改未能保留目标结构".to_owned());
    }
    Ok(output)
}

fn object_value(source: &str, object: &JsonObjectSpan) -> Result<JsonValue, String> {
    object
        .members
        .iter()
        .map(|member| {
            Ok((
                member.key.clone(),
                span_value(
                    source,
                    member.value_start,
                    member.value_end,
                    member.object.as_ref(),
                    member.array.as_ref(),
                )?,
            ))
        })
        .collect::<Result<serde_json::Map<_, _>, String>>()
        .map(JsonValue::Object)
}

fn span_value(
    source: &str,
    start: usize,
    end: usize,
    object: Option<&JsonObjectSpan>,
    array: Option<&JsonArraySpan>,
) -> Result<JsonValue, String> {
    if let Some(object) = object {
        return object_value(source, object);
    }
    if let Some(array) = array {
        return array
            .members
            .iter()
            .map(|member| {
                span_value(
                    source,
                    member.start,
                    member.end,
                    member.object.as_ref(),
                    member.array.as_ref(),
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map(JsonValue::Array);
    }
    serde_json::from_str(&source[start..end]).map_err(|_| "JSON 配置格式无效".to_owned())
}

fn render_value(
    source: &str,
    start: usize,
    end: usize,
    object: Option<&JsonObjectSpan>,
    array: Option<&JsonArraySpan>,
    desired: &JsonValue,
) -> Result<String, String> {
    if span_value(source, start, end, object, array)? == *desired {
        return Ok(source[start..end].to_owned());
    }
    if let (Some(object), Some(value)) = (object, desired.as_object()) {
        return render_object(source, object, value);
    }
    if let (Some(array), Some(values)) = (array, desired.as_array()) {
        return render_array(source, array, values);
    }
    serde_json::to_string(desired).map_err(|_| "无法生成 JSON 配置字段".to_owned())
}

struct InlineMemberFormatter;

impl serde_json::ser::Formatter for InlineMemberFormatter {
    fn begin_array_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_key<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
    ) -> std::io::Result<()> {
        writer.write_all(b": ")
    }
}

fn render_inserted_value(
    source: &str,
    parent: &JsonObjectSpan,
    value: &JsonValue,
) -> Result<String, String> {
    let multiline = is_multiline(source, parent);
    let mut output = Vec::new();
    let failure = |_| "无法生成 JSON 配置字段".to_owned();
    if multiline {
        let indent = indentation_unit(source);
        let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
        value
            .serialize(&mut serde_json::Serializer::with_formatter(
                &mut output,
                formatter,
            ))
            .map_err(failure)?;
    } else {
        value
            .serialize(&mut serde_json::Serializer::with_formatter(
                &mut output,
                InlineMemberFormatter,
            ))
            .map_err(failure)?;
    }
    let rendered = String::from_utf8(output).map_err(|_| "无法生成 JSON 配置字段".to_owned())?;
    if multiline {
        // JSON string newlines are escaped by the serializer; only its layout
        // newlines receive the existing native line ending and member indent.
        let newline = format!("{}{}", newline_for(source), member_indent(source, parent));
        Ok(rendered.replace('\n', &newline))
    } else {
        Ok(rendered)
    }
}

fn render_object(
    source: &str,
    object: &JsonObjectSpan,
    desired: &serde_json::Map<String, JsonValue>,
) -> Result<String, String> {
    let values = object
        .members
        .iter()
        .map(|member| {
            desired
                .get(&member.key)
                .map(|value| {
                    let rendered = render_value(
                        source,
                        member.value_start,
                        member.value_end,
                        member.object.as_ref(),
                        member.array.as_ref(),
                        value,
                    )?;
                    Ok(format!(
                        "{}{rendered}",
                        &source[member.key_start..member.value_start]
                    ))
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut insertions = vec![Vec::new(); values.len() + 1];
    for (key, value) in desired {
        if !object.members.iter().any(|member| member.key == *key) {
            insertions[values.len()].push(format!(
                "{}: {}",
                json_string(key),
                render_inserted_value(source, object, value)?
            ));
        }
    }
    let spans = object
        .members
        .iter()
        .map(|member| (member.key_start, member.value_end))
        .collect::<Vec<_>>();
    Ok(render_container(
        source,
        object.open,
        object.close,
        &object.commas,
        &spans,
        values,
        insertions,
    ))
}

fn render_array(
    source: &str,
    array: &JsonArraySpan,
    desired: &[JsonValue],
) -> Result<String, String> {
    let original = array
        .members
        .iter()
        .map(|member| {
            span_value(
                source,
                member.start,
                member.end,
                member.object.as_ref(),
                member.array.as_ref(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    // Match unchanged neighbours before pairing replacements. This prevents an
    // earlier removal from reformatting or rewriting every later handler.
    let (assignment, additions) = align_array(&original, desired)?;
    let values = array
        .members
        .iter()
        .zip(assignment)
        .map(|(member, index)| {
            index
                .map(|index| {
                    render_value(
                        source,
                        member.start,
                        member.end,
                        member.object.as_ref(),
                        member.array.as_ref(),
                        &desired[index],
                    )
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let insertions = additions
        .into_iter()
        .map(|indices| {
            indices
                .into_iter()
                .map(|index| {
                    serde_json::to_string(&desired[index])
                        .map_err(|_| "无法生成 JSON 数组项".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let spans = array
        .members
        .iter()
        .map(|member| (member.start, member.end))
        .collect::<Vec<_>>();
    Ok(render_container(
        source,
        array.open,
        array.close,
        &array.commas,
        &spans,
        values,
        insertions,
    ))
}

type ArrayAlignment = (Vec<Option<usize>>, Vec<Vec<usize>>);

pub(crate) fn align_array(
    original: &[JsonValue],
    desired: &[JsonValue],
) -> Result<ArrayAlignment, String> {
    let rows = original.len() + 1;
    let columns = desired.len() + 1;
    if rows
        .checked_mul(columns)
        .is_none_or(|size| size > 4_194_304)
    {
        return Err("JSON 数组修改超过有界处理范围".to_owned());
    }
    let mut lengths = vec![0_u32; rows * columns];
    for i in (0..original.len()).rev() {
        for j in (0..desired.len()).rev() {
            lengths[i * columns + j] = if original[i] == desired[j] {
                1 + lengths[(i + 1) * columns + j + 1]
            } else {
                lengths[(i + 1) * columns + j].max(lengths[i * columns + j + 1])
            };
        }
    }
    let mut anchors = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < original.len() && j < desired.len() {
        if original[i] == desired[j] {
            anchors.push((i, j));
            i += 1;
            j += 1;
        } else if lengths[(i + 1) * columns + j] >= lengths[i * columns + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    anchors.push((original.len(), desired.len()));
    let mut assignment = vec![None; original.len()];
    let mut additions = vec![Vec::new(); original.len() + 1];
    let (mut before, mut after) = (0, 0);
    for (old_anchor, new_anchor) in anchors {
        let paired = (old_anchor - before).min(new_anchor - after);
        for offset in 0..paired {
            assignment[before + offset] = Some(after + offset);
        }
        additions[old_anchor].extend(after + paired..new_anchor);
        if old_anchor < original.len() {
            assignment[old_anchor] = Some(new_anchor);
        }
        before = old_anchor + 1;
        after = new_anchor + 1;
    }
    Ok((assignment, additions))
}

fn without_commas(source: &str, start: usize, end: usize, commas: &[usize]) -> String {
    let mut result = String::new();
    let mut cursor = start;
    for comma in commas
        .iter()
        .copied()
        .filter(|comma| *comma >= start && *comma < end)
    {
        result.push_str(&source[cursor..comma]);
        cursor = comma + 1;
    }
    result.push_str(&source[cursor..end]);
    result
}

fn render_container(
    source: &str,
    open: usize,
    close: usize,
    commas: &[usize],
    spans: &[(usize, usize)],
    values: Vec<Option<String>>,
    mut insertions: Vec<Vec<String>>,
) -> String {
    let multiline = source[open..close].contains('\n');
    let indent = spans
        .first()
        .map(|(start, _)| line_indent_at(source, *start))
        .unwrap_or_else(|| {
            format!(
                "{}{}",
                line_indent_at(source, open),
                indentation_unit(source)
            )
        });
    let fresh_gap = if multiline {
        format!("{}{indent}", newline_for(source))
    } else {
        " ".to_owned()
    };
    let mut output = source[open..open + 1].to_owned();
    let mut pending = String::new();
    let mut emitted = false;
    let mut cursor = open + 1;
    let append = |output: &mut String, pending: &mut String, emitted: &mut bool, value: &str| {
        if *emitted {
            output.push(',');
        }
        output.push_str(pending);
        pending.clear();
        output.push_str(value);
        *emitted = true;
    };
    for (i, &(start, end)) in spans.iter().enumerate() {
        pending.push_str(&without_commas(source, cursor, start, commas));
        for value in &insertions[i] {
            if pending.is_empty() {
                pending.push_str(&fresh_gap);
            }
            append(&mut output, &mut pending, &mut emitted, value);
            pending.push_str(&fresh_gap);
        }
        if let Some(value) = &values[i] {
            append(&mut output, &mut pending, &mut emitted, value);
        }
        cursor = end;
    }
    let appended = !insertions[spans.len()].is_empty();
    if spans.is_empty() && appended {
        pending.push_str(&source[open + 1..close]);
        if multiline {
            pending.push_str(&indentation_unit(source));
        }
        cursor = close;
    }
    for value in insertions.pop().unwrap_or_default() {
        if pending.is_empty() && (emitted || multiline) {
            pending.push_str(&fresh_gap);
        }
        append(&mut output, &mut pending, &mut emitted, &value);
    }
    if emitted
        && commas
            .last()
            .is_some_and(|comma| spans.last().is_some_and(|(_, end)| comma >= end))
    {
        output.push(',');
    }
    pending.push_str(&without_commas(source, cursor, close, commas));
    if spans.is_empty() && appended && multiline {
        pending.push_str(newline_for(source));
        pending.push_str(&line_indent_at(source, close));
    }
    output.push_str(&pending);
    output.push_str(&source[close..close + 1]);
    output
}

#[cfg(test)]
mod document_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_batch_array_edits_preserve_comments_and_untouched_values() {
        let source = "{\r\n  // retain policy\r\n  \"policy\": {\"x\": 1},\r\n  \"hooks\": [\r\n    {\"id\":1}, // first\r\n    { \"id\" : 2, \"unknown\": true }, /* neighbour */\r\n    {\"id\":3}\r\n  ]\r\n}\r\n";
        let desired = json!({"policy":{"x":1},"hooks":[{"id":2,"unknown":true},{"id":4}]});
        let output = rewrite_json_document(source, &desired, true).unwrap();
        assert_eq!(parse_jsonc_document(&output).unwrap(), desired);
        assert!(output.contains("{ \"id\" : 2, \"unknown\": true }"));
        for comment in ["// retain policy", "// first", "/* neighbour */"] {
            assert!(output.contains(comment));
        }
        assert!(output.ends_with("}\r\n"));
        assert!(rewrite_json_document(source, &desired, false).is_err());
    }

    #[test]
    fn duplicate_keys_and_unterminated_comments_are_rejected() {
        for source in [
            "{\"a\":1,\"a\":2}",
            "{\"a\":[{\"x\":1,\"x\":2}]}",
            "{/* missing}",
            "{\"a\":1,}",
            "{\"a\":[1, /* no trailing comma */ ]}",
        ] {
            assert!(parse_jsonc_document(source).is_err());
        }
    }

    #[test]
    fn inserts_missing_members_without_losing_empty_container_comments() {
        let source = "{\n  \"hooks\": { /* group guidance */ },\n  \"policy\": true\n}";
        let desired = json!({"hooks":{"BeforeTool":[{"hooks":[{"type":"command","command":"fixture"}]}]},"policy":true});
        let output = rewrite_json_document(source, &desired, true).unwrap();
        assert!(output.contains("/* group guidance */"));
        assert_eq!(parse_jsonc_document(&output).unwrap(), desired);
    }
}
