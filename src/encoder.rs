use serde_json::{Map, Value};
use std::fmt::Write;

#[derive(Clone, Copy)]
pub enum Delimiter {
    Comma,
    Tab,
    Pipe,
}

impl Delimiter {
    fn sep(self) -> &'static str {
        match self {
            Delimiter::Comma => ",",
            Delimiter::Tab => "\t",
            Delimiter::Pipe => "|",
        }
    }

    fn bracket_sym(self) -> &'static str {
        match self {
            Delimiter::Comma => "",
            Delimiter::Tab => "\t",
            Delimiter::Pipe => "|",
        }
    }
}

pub struct Encoder {
    indent: usize,
    delim: Delimiter,
}

impl Encoder {
    pub fn new(indent: usize, delim: Delimiter) -> Self {
        Self { indent, delim }
    }

    pub fn encode(&self, value: &Value) -> String {
        let mut out = String::new();
        match value {
            Value::Object(map) => {
                if !map.is_empty() {
                    self.write_fields(&mut out, map, 0);
                }
            }
            Value::Array(arr) => {
                self.write_array(&mut out, "", arr, 0);
            }
            _ => {
                write!(out, "{}", self.fmt_primitive(value)).unwrap();
            }
        }
        out
    }

    fn pad(&self, depth: usize) -> String {
        " ".repeat(depth * self.indent)
    }

    /// Write object fields at given depth.
    fn write_fields(&self, out: &mut String, map: &Map<String, Value>, depth: usize) {
        let pad = self.pad(depth);
        for (i, (key, val)) in map.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let k = quote_key(key);
            match val {
                Value::Object(inner) => {
                    write!(out, "{}{}:", pad, k).unwrap();
                    if !inner.is_empty() {
                        out.push('\n');
                        self.write_fields(out, inner, depth + 1);
                    }
                }
                Value::Array(arr) => {
                    self.write_array(out, &k, arr, depth);
                }
                _ => {
                    write!(out, "{}{}: {}", pad, k, self.fmt_primitive(val)).unwrap();
                }
            }
        }
    }

    /// Write an array field (inline, tabular, or expanded).
    fn write_array(&self, out: &mut String, key: &str, arr: &[Value], depth: usize) {
        let pad = self.pad(depth);
        let sym = self.delim.bracket_sym();
        let sep = self.delim.sep();

        if arr.is_empty() {
            write!(out, "{}{}[0{}]:", pad, key, sym).unwrap();
            return;
        }

        // All primitives → inline
        if arr.iter().all(is_primitive) {
            let vals: Vec<String> = arr.iter().map(|v| self.fmt_primitive(v)).collect();
            write!(
                out,
                "{}{}[{}{}]: {}",
                pad,
                key,
                arr.len(),
                sym,
                vals.join(sep)
            )
            .unwrap();
            return;
        }

        // Tabular: all objects, same keys, all primitive values
        if let Some(fields) = tabular_fields(arr) {
            let hdr: Vec<String> = fields.iter().map(|f| quote_key(f)).collect();
            write!(
                out,
                "{}{}[{}{}]{{{}}}:",
                pad,
                key,
                arr.len(),
                sym,
                hdr.join(sep)
            )
            .unwrap();
            let row_pad = self.pad(depth + 1);
            for item in arr {
                let obj = item.as_object().unwrap();
                let vals: Vec<String> = fields
                    .iter()
                    .map(|f| self.fmt_primitive(obj.get(f).unwrap()))
                    .collect();
                write!(out, "\n{}{}", row_pad, vals.join(sep)).unwrap();
            }
            return;
        }

        // Expanded list (mixed types, arrays of arrays, non-uniform objects)
        write!(out, "{}{}[{}{}]:", pad, key, arr.len(), sym).unwrap();
        for item in arr {
            out.push('\n');
            self.write_list_item(out, item, depth + 1);
        }
    }

    /// Write a `- value` list item.
    fn write_list_item(&self, out: &mut String, value: &Value, depth: usize) {
        let pad = self.pad(depth);
        match value {
            Value::Object(map) => {
                if map.is_empty() {
                    write!(out, "{}-", pad).unwrap();
                    return;
                }
                let mut iter = map.iter();
                let (fk, fv) = iter.next().unwrap();
                let k = quote_key(fk);

                // First field on hyphen line
                match fv {
                    Value::Object(inner) => {
                        write!(out, "{}- {}:", pad, k).unwrap();
                        if !inner.is_empty() {
                            out.push('\n');
                            self.write_fields(out, inner, depth + 2);
                        }
                    }
                    Value::Array(arr) => {
                        self.write_hyphen_array(out, &pad, &k, arr, depth);
                    }
                    _ => {
                        write!(out, "{}- {}: {}", pad, k, self.fmt_primitive(fv)).unwrap();
                    }
                }

                // Remaining fields at depth+1
                let rest_pad = self.pad(depth + 1);
                for (rk, rv) in iter {
                    out.push('\n');
                    let k = quote_key(rk);
                    match rv {
                        Value::Object(inner) => {
                            write!(out, "{}{}:", rest_pad, k).unwrap();
                            if !inner.is_empty() {
                                out.push('\n');
                                self.write_fields(out, inner, depth + 2);
                            }
                        }
                        Value::Array(arr) => {
                            self.write_array(out, &k, arr, depth + 1);
                        }
                        _ => {
                            write!(out, "{}{}: {}", rest_pad, k, self.fmt_primitive(rv)).unwrap();
                        }
                    }
                }
            }
            Value::Array(arr) => {
                let sym = self.delim.bracket_sym();
                let sep = self.delim.sep();
                if arr.is_empty() {
                    write!(out, "{}- [0{}]:", pad, sym).unwrap();
                } else if arr.iter().all(is_primitive) {
                    let vals: Vec<String> = arr.iter().map(|v| self.fmt_primitive(v)).collect();
                    write!(out, "{}- [{}{}]: {}", pad, arr.len(), sym, vals.join(sep)).unwrap();
                } else {
                    write!(out, "{}- [{}{}]:", pad, arr.len(), sym).unwrap();
                    for item in arr {
                        out.push('\n');
                        self.write_list_item(out, item, depth + 1);
                    }
                }
            }
            _ => {
                write!(out, "{}- {}", pad, self.fmt_primitive(value)).unwrap();
            }
        }
    }

    /// First field of a hyphen-line object is an array.
    fn write_hyphen_array(
        &self,
        out: &mut String,
        pad: &str,
        key: &str,
        arr: &[Value],
        depth: usize,
    ) {
        let sym = self.delim.bracket_sym();
        let sep = self.delim.sep();

        if arr.is_empty() {
            write!(out, "{}- {}[0{}]:", pad, key, sym).unwrap();
            return;
        }

        if arr.iter().all(is_primitive) {
            let vals: Vec<String> = arr.iter().map(|v| self.fmt_primitive(v)).collect();
            write!(
                out,
                "{}- {}[{}{}]: {}",
                pad,
                key,
                arr.len(),
                sym,
                vals.join(sep)
            )
            .unwrap();
            return;
        }

        if let Some(fields) = tabular_fields(arr) {
            let hdr: Vec<String> = fields.iter().map(|f| quote_key(f)).collect();
            write!(
                out,
                "{}- {}[{}{}]{{{}}}:",
                pad,
                key,
                arr.len(),
                sym,
                hdr.join(sep)
            )
            .unwrap();
            let row_pad = self.pad(depth + 2);
            for item in arr {
                let obj = item.as_object().unwrap();
                let vals: Vec<String> = fields
                    .iter()
                    .map(|f| self.fmt_primitive(obj.get(f).unwrap()))
                    .collect();
                write!(out, "\n{}{}", row_pad, vals.join(sep)).unwrap();
            }
            return;
        }

        write!(out, "{}- {}[{}{}]:", pad, key, arr.len(), sym).unwrap();
        for item in arr {
            out.push('\n');
            self.write_list_item(out, item, depth + 2);
        }
    }

    fn fmt_primitive(&self, value: &Value) -> String {
        match value {
            Value::Null => "null".into(),
            Value::Bool(b) => if *b { "true" } else { "false" }.into(),
            Value::Number(n) => format_number(n),
            Value::String(s) => quote_value(s, self.delim),
            _ => unreachable!("not a primitive"),
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn is_primitive(v: &Value) -> bool {
    matches!(
        v,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

/// Returns the shared field names if all array elements are objects with
/// identical keys and all-primitive values (tabular eligible).
fn tabular_fields(arr: &[Value]) -> Option<Vec<String>> {
    let first = arr.first()?.as_object()?;
    if !first.values().all(is_primitive) {
        return None;
    }
    let keys: Vec<String> = first.keys().cloned().collect();
    for item in &arr[1..] {
        let obj = item.as_object()?;
        if obj.len() != keys.len() {
            return None;
        }
        for k in &keys {
            if !obj.get(k).map(is_primitive).unwrap_or(false) {
                return None;
            }
        }
    }
    Some(keys)
}

// ── key / string encoding ───────────────────────────────────────────────────

fn quote_key(key: &str) -> String {
    if is_safe_key(key) {
        key.into()
    } else {
        format!("\"{}\"", escape(key))
    }
}

fn is_safe_key(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn quote_value(s: &str, delim: Delimiter) -> String {
    if needs_quoting(s, delim) {
        format!("\"{}\"", escape(s))
    } else {
        s.into()
    }
}

fn needs_quoting(s: &str, delim: Delimiter) -> bool {
    if s.is_empty() {
        return true;
    }
    if s != s.trim() {
        return true;
    }
    if matches!(s, "true" | "false" | "null") {
        return true;
    }
    if s.starts_with('-') {
        return true;
    }
    if is_numeric_like(s) {
        return true;
    }
    if s.contains(|c: char| matches!(c, ':' | '"' | '\\' | '[' | ']' | '{' | '}')) {
        return true;
    }
    if s.contains(|c: char| c == '\n' || c == '\r' || c == '\t') {
        return true;
    }
    if s.contains(delim.sep()) {
        return true;
    }
    false
}

fn is_numeric_like(s: &str) -> bool {
    let b = s.as_bytes();
    // Leading-zero integer like "05"
    if b.len() > 1 && b[0] == b'0' && b[1].is_ascii_digit() {
        return true;
    }
    // General: -?\d+(\.\d+)?([eE][+-]?\d+)?
    let mut i = 0;
    if i < b.len() && b[i] == b'-' {
        i += 1;
    }
    if i >= b.len() || !b[i].is_ascii_digit() {
        return false;
    }
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        if i >= b.len() || !b[i].is_ascii_digit() {
            return false;
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if i >= b.len() || !b[i].is_ascii_digit() {
            return false;
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    i == b.len()
}

// ── number encoding ─────────────────────────────────────────────────────────

fn format_number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    if let Some(f) = n.as_f64() {
        if f.is_nan() || f.is_infinite() {
            return "null".into();
        }
        if f == 0.0 {
            return "0".into();
        }
        if f.fract() == 0.0 && f.abs() < (i64::MAX as f64) {
            return (f as i64).to_string();
        }
        let s = format!("{}", f);
        if s.contains('e') || s.contains('E') {
            return expand_exponent(&s);
        }
        strip_trailing_zeros(&s)
    } else {
        n.to_string()
    }
}

fn strip_trailing_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.into();
    }
    s.trim_end_matches('0').trim_end_matches('.').into()
}

fn expand_exponent(s: &str) -> String {
    let lower = s.to_lowercase();
    let parts: Vec<&str> = lower.split('e').collect();
    if parts.len() != 2 {
        return s.into();
    }
    let neg = parts[0].starts_with('-');
    let mantissa = parts[0].trim_start_matches('-');
    let exp: i32 = parts[1].parse().unwrap_or(0);

    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa, ""),
    };

    let digits = format!("{}{}", int_part, frac_part);
    let dot_pos = int_part.len() as i32 + exp;

    let result = if dot_pos <= 0 {
        let zeros = (-dot_pos) as usize;
        format!("0.{}{}", "0".repeat(zeros), digits)
    } else if dot_pos as usize >= digits.len() {
        let extra = dot_pos as usize - digits.len();
        format!("{}{}", digits, "0".repeat(extra))
    } else {
        let pos = dot_pos as usize;
        format!("{}.{}", &digits[..pos], &digits[pos..])
    };

    let result = strip_trailing_zeros(&result);
    if neg {
        format!("-{}", result)
    } else {
        result
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(json: &str) -> String {
        let v: Value = serde_json::from_str(json).unwrap();
        Encoder::new(2, Delimiter::Comma).encode(&v)
    }

    #[test]
    fn spec_example() {
        let json = r#"{
            "context": {
                "task": "Our favorite hikes together",
                "location": "Boulder",
                "season": "spring_2025"
            },
            "friends": ["ana", "luis", "sam"],
            "hikes": [
                {"id": 1, "name": "Blue Lake Trail", "distanceKm": 7.5, "elevationGain": 320, "companion": "ana", "wasSunny": true},
                {"id": 2, "name": "Ridge Overlook", "distanceKm": 9.2, "elevationGain": 540, "companion": "luis", "wasSunny": false},
                {"id": 3, "name": "Wildflower Loop", "distanceKm": 5.1, "elevationGain": 180, "companion": "sam", "wasSunny": true}
            ]
        }"#;
        let expected = "context:\n  task: Our favorite hikes together\n  location: Boulder\n  season: spring_2025\nfriends[3]: ana,luis,sam\nhikes[3]{id,name,distanceKm,elevationGain,companion,wasSunny}:\n  1,Blue Lake Trail,7.5,320,ana,true\n  2,Ridge Overlook,9.2,540,luis,false\n  3,Wildflower Loop,5.1,180,sam,true";
        assert_eq!(enc(json), expected);
    }

    #[test]
    fn empty_object() {
        assert_eq!(enc("{}"), "");
    }

    #[test]
    fn empty_array() {
        assert_eq!(enc("[]"), "[0]:");
    }

    #[test]
    fn single_primitive() {
        assert_eq!(enc("42"), "42");
        assert_eq!(enc("\"hello\""), "hello");
        assert_eq!(enc("true"), "true");
        assert_eq!(enc("null"), "null");
    }

    #[test]
    fn string_quoting() {
        assert_eq!(enc(r#"{"x": "true"}"#), "x: \"true\"");
        assert_eq!(enc(r#"{"x": "a:b"}"#), "x: \"a:b\"");
        assert_eq!(enc(r#"{"x": ""}"#), "x: \"\"");
        assert_eq!(enc(r#"{"x": "-foo"}"#), "x: \"-foo\"");
        assert_eq!(enc(r#"{"x": "42"}"#), "x: \"42\"");
    }

    #[test]
    fn nested_objects() {
        assert_eq!(enc(r#"{"a": {"b": {"c": 1}}}"#), "a:\n  b:\n    c: 1");
    }

    #[test]
    fn primitive_array() {
        assert_eq!(enc(r#"{"n": [1, 2, 3]}"#), "n[3]: 1,2,3");
    }

    #[test]
    fn expanded_list() {
        let json = r#"{"items": [1, {"a": 1}, "text"]}"#;
        assert_eq!(enc(json), "items[3]:\n  - 1\n  - a: 1\n  - text");
    }

    #[test]
    fn array_of_arrays() {
        let json = r#"{"pairs": [[1, 2], [3, 4]]}"#;
        assert_eq!(enc(json), "pairs[2]:\n  - [2]: 1,2\n  - [2]: 3,4");
    }

    #[test]
    fn tabular_objects() {
        let json = r#"{"users": [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]}"#;
        assert_eq!(
            enc(json),
            "users[2]{id,name}:\n  1,Alice\n  2,Bob"
        );
    }

    #[test]
    fn key_quoting() {
        assert_eq!(enc(r#"{"valid_key.x": 1}"#), "valid_key.x: 1");
        assert_eq!(enc(r#"{"has space": 1}"#), "\"has space\": 1");
        assert_eq!(enc(r#"{"": 1}"#), "\"\": 1");
        assert_eq!(enc(r#"{"123": 1}"#), "\"123\": 1");
    }

    #[test]
    fn root_array() {
        assert_eq!(enc("[1,2,3]"), "[3]: 1,2,3");
    }

    #[test]
    fn number_canonical() {
        assert_eq!(enc("0"), "0");
        assert_eq!(enc("7.5"), "7.5");
        assert_eq!(enc("-3"), "-3");
    }

    #[test]
    fn object_list_items() {
        let json = r#"{"items": [{"id": 1, "name": "First"}, {"id": 2, "sub": {"x": 1}}]}"#;
        let expected = "items[2]:\n  - id: 1\n    name: First\n  - id: 2\n    sub:\n      x: 1";
        assert_eq!(enc(json), expected);
    }

    #[test]
    fn empty_nested_object() {
        assert_eq!(enc(r#"{"a": {}}"#), "a:");
    }

    #[test]
    fn string_with_delimiter() {
        assert_eq!(enc(r#"{"x": "a,b"}"#), "x: \"a,b\"");
    }
}
