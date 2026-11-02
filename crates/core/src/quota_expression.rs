//! 余额接口表达式：只做 JSON 取值、算术和少量聚合，不执行任意代码。
//!
//! 语法与 `magpie` 的余额表达式保持同一套直觉：`data.balance`、`a.*.used`、
//! `a[status=active].used`、`sum(a.*.used)`、`(total - balance) / total * 100`。
//! 求值失败只影响当前字段，调用方可以选择跳过窗口或把错误写进同步状态。

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
enum Segment {
    Key(String),
    Index(usize),
    Wildcard,
    Filter(String, String),
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Number(f64),
    Path(Vec<Segment>),
    /// 保留原始写法，便于错误信息定位到字段而不是内部 AST。
    Unary(char, Box<Expr>),
    Binary(char, Box<Expr>, Box<Expr>),
    Percent(Box<Expr>),
    Call(String, Vec<Expr>),
}

pub fn validate_expression(expression: &str) -> Result<(), String> {
    parse(expression).map(|_| ())
}

/// 求出单个数值。路径解析出多个值时视为错误，列表场景请显式套聚合函数。
pub fn evaluate(expression: &str, root: &Value) -> Result<f64, String> {
    let expr = parse(expression)?;
    let mut values = Vec::new();
    eval(&expr, root, &mut values)?;
    match values.as_slice() {
        [value] => number(value),
        [] => Err("表达式没有取到值".to_string()),
        _ => Err("表达式取到多个值，请改用 sum / max / min / count".to_string()),
    }
}

/// 时间字段：数字按秒或毫秒识别，字符串按 RFC 3339 或常见日期时间格式解析。
pub fn evaluate_timestamp(expression: &str, root: &Value) -> Result<i64, String> {
    let expr = parse(expression)?;
    if let Expr::Path(segments) = &expr {
        let values = resolve(segments, root);
        if let [value] = values.as_slice() {
            if let Some(timestamp) = timestamp(value) {
                return Ok(timestamp);
            }
            return Err("时间字段不是可识别的时间".to_string());
        }
    }
    let seconds = evaluate(expression, root)?;
    Ok(normalize_epoch(seconds))
}

fn normalize_epoch(value: f64) -> i64 {
    let value = value as i64;
    if value.abs() > 100_000_000_000 {
        value / 1000
    } else {
        value
    }
}

fn timestamp(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_f64().map(normalize_epoch),
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text) {
                return Some(parsed.timestamp());
            }
            for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d"] {
                if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(text, format) {
                    return Some(parsed.and_utc().timestamp());
                }
                if let Ok(parsed) = chrono::NaiveDate::parse_from_str(text, format) {
                    return Some(parsed.and_hms_opt(0, 0, 0)?.and_utc().timestamp());
                }
            }
            text.parse::<f64>().ok().map(normalize_epoch)
        }
        _ => None,
    }
}

fn number(value: &Value) -> Result<f64, String> {
    match value {
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| "数值超出可表示范围".to_string()),
        Value::String(text) => text
            .trim()
            .trim_end_matches('%')
            .parse::<f64>()
            .map_err(|_| format!("「{text}」不是数字")),
        _ => Err("取到的值不是数字".to_string()),
    }
}

fn eval(expr: &Expr, root: &Value, out: &mut Vec<Value>) -> Result<(), String> {
    match expr {
        Expr::Number(value) => {
            out.push(Value::from(*value));
            Ok(())
        }
        Expr::Path(segments) => {
            out.extend(resolve(segments, root));
            Ok(())
        }
        Expr::Unary(_, inner) => {
            let value = eval_number(inner, root)?;
            out.push(Value::from(-value));
            Ok(())
        }
        Expr::Percent(inner) => {
            let value = eval_number(inner, root)?;
            out.push(Value::from(value * 100.0));
            Ok(())
        }
        Expr::Binary(op, left, right) => {
            let left = eval_number(left, root)?;
            let right = eval_number(right, root)?;
            let value = match op {
                '+' => left + right,
                '-' => left - right,
                '*' => left * right,
                '/' => {
                    if right == 0.0 {
                        return Err("表达式除以 0".to_string());
                    }
                    left / right
                }
                _ => return Err("不支持的运算符".to_string()),
            };
            out.push(Value::from(value));
            Ok(())
        }
        Expr::Call(name, args) => {
            let values = if let Some(Expr::Path(segments)) = args.first() {
                resolve(segments, root)
            } else {
                let mut values = Vec::new();
                for arg in args {
                    eval(arg, root, &mut values)?;
                }
                values
            };
            if values.is_empty() {
                return Err("表达式没有取到值".to_string());
            }
            let numbers = values
                .iter()
                .filter_map(|value| number(value).ok())
                .collect::<Vec<_>>();
            if name != "count" && numbers.is_empty() {
                return Err(format!("{name} 没有可聚合的数值"));
            }
            let value = match name.as_str() {
                "count" => values.len() as f64,
                "sum" => numbers.iter().sum(),
                "max" => numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                "min" => numbers.iter().copied().fold(f64::INFINITY, f64::min),
                "avg" => numbers.iter().sum::<f64>() / numbers.len() as f64,
                other => return Err(format!("不支持的函数 {other}")),
            };
            if !value.is_finite() {
                return Err(format!("{name} 没有可聚合的数值"));
            }
            out.push(Value::from(value));
            Ok(())
        }
    }
}

fn eval_number(expr: &Expr, root: &Value) -> Result<f64, String> {
    let mut values = Vec::new();
    eval(expr, root, &mut values)?;
    match values.as_slice() {
        [value] => number(value),
        [] => Err("表达式没有取到值".to_string()),
        _ => Err("表达式取到多个值，请改用 sum / max / min / count".to_string()),
    }
}

fn resolve(segments: &[Segment], root: &Value) -> Vec<Value> {
    let mut current = vec![root.clone()];
    for segment in segments {
        let mut next = Vec::new();
        for value in current {
            match segment {
                Segment::Key(key) => {
                    if let Some(found) = value.get(key) {
                        next.push(found.clone());
                    }
                }
                Segment::Index(index) => {
                    if let Some(found) = value.get(*index) {
                        next.push(found.clone());
                    }
                }
                Segment::Wildcard => match value {
                    Value::Array(items) => next.extend(items.iter().cloned()),
                    Value::Object(map) => next.extend(map.values().cloned()),
                    _ => {}
                },
                Segment::Filter(key, expected) => match &value {
                    Value::Array(items) => next.extend(
                        items
                            .iter()
                            .filter(|item| {
                                item.get(key)
                                    .and_then(scalar_text)
                                    .is_some_and(|text| text == *expected)
                            })
                            .cloned(),
                    ),
                    Value::Object(map) => {
                        let matches = map
                            .get(key)
                            .and_then(scalar_text)
                            .is_some_and(|text| text == *expected);
                        if matches {
                            next.push(value.clone());
                        }
                    }
                    _ => {}
                },
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

struct Parser<'a> {
    input: &'a [u8],
    text: &'a str,
    pos: usize,
}

fn parse(input: &str) -> Result<Expr, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("表达式为空".to_string());
    }
    let mut parser = Parser {
        input: input.as_bytes(),
        text: input,
        pos: 0,
    };
    let expr = parser.expression()?;
    parser.skip_space();
    if parser.pos != parser.input.len() {
        return Err(format!(
            "表达式在「{}」处无法继续解析",
            &parser.text[parser.pos..]
        ));
    }
    Ok(expr)
}

impl Parser<'_> {
    fn expression(&mut self) -> Result<Expr, String> {
        let mut left = self.term()?;
        loop {
            self.skip_space();
            match self.peek() {
                Some(b'+') => {
                    self.pos += 1;
                    let right = self.term()?;
                    left = Expr::Binary('+', Box::new(left), Box::new(right));
                }
                Some(b'-') => {
                    self.pos += 1;
                    let right = self.term()?;
                    left = Expr::Binary('-', Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            self.skip_space();
            match self.peek() {
                Some(b'*') => {
                    self.pos += 1;
                    let right = self.unary()?;
                    left = Expr::Binary('*', Box::new(left), Box::new(right));
                }
                Some(b'/') => {
                    self.pos += 1;
                    let right = self.unary()?;
                    left = Expr::Binary('/', Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        self.skip_space();
        if self.peek() == Some(b'-') {
            self.pos += 1;
            return Ok(Expr::Unary('-', Box::new(self.unary()?)));
        }
        if self.peek() == Some(b'+') {
            self.pos += 1;
            return self.unary();
        }
        let mut value = self.primary()?;
        self.skip_space();
        if self.peek() == Some(b'%') {
            self.pos += 1;
            value = Expr::Percent(Box::new(value));
        }
        Ok(value)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        self.skip_space();
        match self.peek() {
            Some(b'(') => {
                self.pos += 1;
                let expr = self.expression()?;
                self.skip_space();
                if self.peek() != Some(b')') {
                    return Err("缺少右括号".to_string());
                }
                self.pos += 1;
                Ok(expr)
            }
            Some(b) if b.is_ascii_digit() || b == b'.' => self.number(),
            Some(b) if is_ident_start(b) => self.ident_or_path(),
            Some(_) => Err(format!("无法识别的字符「{}」", self.current_char())),
            None => Err("表达式不完整".to_string()),
        }
    }

    fn number(&mut self) -> Result<Expr, String> {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit() || b == b'.') {
            self.pos += 1;
        }
        self.text[start..self.pos]
            .parse::<f64>()
            .map(Expr::Number)
            .map_err(|_| "数字格式不正确".to_string())
    }

    fn ident_or_path(&mut self) -> Result<Expr, String> {
        let start = self.pos;
        while self.peek().is_some_and(is_ident_char) {
            self.pos += 1;
        }
        let name = &self.text[start..self.pos];
        self.skip_space();
        if self.peek() == Some(b'(') && name.chars().all(|c| c.is_ascii_alphabetic()) {
            if !matches!(name, "sum" | "max" | "min" | "avg" | "count") {
                return Err(format!("不支持的函数 {name}"));
            }
            self.pos += 1;
            let args = self.arguments()?;
            return Ok(Expr::Call(name.to_string(), args));
        }
        let mut segments = vec![Segment::Key(name.to_string())];
        loop {
            match self.peek() {
                Some(b'.') => {
                    self.pos += 1;
                    segments.push(self.segment()?);
                }
                Some(b'[') => {
                    self.pos += 1;
                    segments.push(self.bracket()?);
                }
                _ => break,
            }
        }
        Ok(Expr::Path(segments))
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        loop {
            self.skip_space();
            if self.peek() == Some(b')') {
                self.pos += 1;
                break;
            }
            args.push(self.expression()?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b')') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err("函数参数之间缺少逗号".to_string()),
            }
        }
        Ok(args)
    }

    fn segment(&mut self) -> Result<Segment, String> {
        self.skip_space();
        match self.peek() {
            Some(b'*') => {
                self.pos += 1;
                Ok(Segment::Wildcard)
            }
            Some(b'"') | Some(b'\'') => self.quoted().map(Segment::Key),
            Some(_) => {
                let start = self.pos;
                while self.peek().is_some_and(is_segment_char) {
                    self.pos += 1;
                }
                if start == self.pos {
                    return Err("路径段为空".to_string());
                }
                Ok(Segment::Key(self.text[start..self.pos].to_string()))
            }
            None => Err("路径不完整".to_string()),
        }
    }

    fn bracket(&mut self) -> Result<Segment, String> {
        self.skip_space();
        let segment = match self.peek() {
            Some(b'*') => {
                self.pos += 1;
                Segment::Wildcard
            }
            Some(b'"') | Some(b'\'') => {
                let key = self.quoted()?;
                self.skip_space();
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    let expected = self.filter_value()?;
                    Segment::Filter(key, expected)
                } else if self.peek() == Some(b']') {
                    Segment::Key(key)
                } else {
                    return Err("方括号里只能写索引、键名或过滤条件".to_string());
                }
            }
            Some(b) if b.is_ascii_digit() => {
                let start = self.pos;
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.pos += 1;
                }
                Segment::Index(self.text[start..self.pos].parse().unwrap_or(0))
            }
            Some(_) => {
                let key = self.segment()?;
                self.skip_space();
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    let expected = self.filter_value()?;
                    let Segment::Key(key) = key else {
                        return Err("过滤条件左侧必须是键名".to_string());
                    };
                    Segment::Filter(key, expected)
                } else {
                    key
                }
            }
            None => return Err("缺少右方括号".to_string()),
        };
        self.skip_space();
        if self.peek() != Some(b']') {
            return Err("缺少右方括号".to_string());
        }
        self.pos += 1;
        Ok(segment)
    }

    fn filter_value(&mut self) -> Result<String, String> {
        self.skip_space();
        if matches!(self.peek(), Some(b'"') | Some(b'\'')) {
            return self.quoted();
        }
        let start = self.pos;
        while self.peek().is_some_and(is_segment_char) {
            self.pos += 1;
        }
        if start == self.pos {
            return Err("过滤条件缺少比较值".to_string());
        }
        Ok(self.text[start..self.pos].to_string())
    }

    fn quoted(&mut self) -> Result<String, String> {
        let quote = self.peek().ok_or_else(|| "缺少引号".to_string())?;
        self.pos += 1;
        let start = self.pos;
        while let Some(byte) = self.peek() {
            if byte == b'\\' {
                self.pos += 2;
                continue;
            }
            if byte == quote {
                let value = self.text[start..self.pos].to_string();
                self.pos += 1;
                return Ok(value);
            }
            self.pos += 1;
        }
        Err("引号没有闭合".to_string())
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn current_char(&self) -> String {
        self.text[self.pos..]
            .chars()
            .next()
            .unwrap_or_default()
            .to_string()
    }
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn is_ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

fn is_segment_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'-' | b':')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data() -> Value {
        json!({
            "data": { "balance": "12.50", "total": 100, "reset_at": "2026-11-01T00:00:00Z" },
            "keys": [
                { "status": "active", "used": 10, "limit": 100 },
                { "status": "expired", "used": 90, "limit": 100 },
                { "status": "active", "used": 30, "limit": 40 }
            ],
            "wallet": { "usd": 3, "cny": 20 }
        })
    }

    #[test]
    fn paths_and_arithmetic_follow_the_declared_shape() {
        let data = data();
        assert_eq!(evaluate("data.balance", &data).unwrap(), 12.5);
        assert_eq!(evaluate("data.total - data.balance", &data).unwrap(), 87.5);
        assert_eq!(
            evaluate("(data.total - data.balance) / data.total * 100", &data).unwrap(),
            87.5
        );
        assert_eq!(evaluate("wallet['usd']", &data).unwrap(), 3.0);
    }

    #[test]
    fn list_selectors_and_aggregates() {
        let data = data();
        assert_eq!(evaluate("sum(keys.*.used)", &data).unwrap(), 130.0);
        assert_eq!(evaluate("max(keys.*.limit)", &data).unwrap(), 100.0);
        assert_eq!(evaluate("count(keys.*.used)", &data).unwrap(), 3.0);
        assert_eq!(
            evaluate("sum(keys[status=active].used)", &data).unwrap(),
            40.0
        );
        assert_eq!(evaluate("keys[0].used", &data).unwrap(), 10.0);
    }

    #[test]
    fn missing_values_fail_without_panicking() {
        let data = data();
        assert!(evaluate("data.missing", &data).is_err());
        assert!(evaluate("keys.*.used", &data).is_err());
        assert!(evaluate("sum(keys.*.missing)", &data).is_err());
        assert!(evaluate("unknown(", &data).is_err());
    }

    #[test]
    fn timestamps_accept_epoch_and_rfc3339() {
        let data = data();
        assert_eq!(
            evaluate_timestamp("data.reset_at", &data).unwrap(),
            1_793_491_200
        );
        assert_eq!(
            evaluate_timestamp("reset_ms", &json!({ "reset_ms": 1_793_491_200_000i64 })).unwrap(),
            1_793_491_200
        );
        assert!(validate_expression("data.balance +").is_err());
        assert!(validate_expression("sum(data.*.used)").is_ok());
    }
}
