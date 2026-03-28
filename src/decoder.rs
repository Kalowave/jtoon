use serde_json::{Map, Number, Value};

// ── public API ──────────────────────────────────────────────────────────────

pub fn decode(input: &str) -> Result<Value, String> {
    let mut parser = Parser::new(input);
    parser.parse_root()
}

// ── types ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Delim {
    Comma,
    Tab,
    Pipe,
}

impl Delim {
    fn char(self) -> char {
        match self {
            Delim::Comma => ',',
            Delim::Tab => '\t',
            Delim::Pipe => '|',
        }
    }
}

struct ArrayHeader {
    key: String,
    #[allow(dead_code)]
    count: usize,
    delim: Delim,
    fields: Option<Vec<String>>,
    inline_values: Option<String>,
}

struct Parser {
    lines: Vec<(usize, String)>, // (leading_spaces, trimmed_content)
    pos: usize,
    indent_size: usize,
}

// ── parser ──────────────────────────────────────────────────────────────────

impl Parser {
    fn new(input: &str) -> Self {
        let lines: Vec<(usize, String)> = input
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let trimmed = l.trim_start_matches(' ');
                let indent = l.len() - trimmed.len();
                (indent, trimmed.to_string())
            })
            .collect();

        let indent_size = lines
            .iter()
            .map(|(i, _)| *i)
            .filter(|&i| i > 0)
            .min()
            .unwrap_or(2);

        Parser {
            lines,
            pos: 0,
            indent_size,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.lines.len()
    }
    fn indent(&self) -> usize {
        self.lines[self.pos].0
    }
    fn content(&self) -> &str {
        &self.lines[self.pos].1
    }

    // ── root ────────────────────────────────────────────────────────────

    fn parse_root(&mut self) -> Result<Value, String> {
        if self.lines.is_empty() {
            return Ok(Value::Object(Map::new()));
        }

        let first = self.lines[0].1.clone();

        // Root array: first depth-0 line is a valid array header
        if first.starts_with('[') {
            if let Some(header) = try_parse_array_header(&first) {
                if header.key.is_empty() {
                    return self.parse_array_body(header, 0);
                }
            }
        }

        // Single primitive: exactly one line, not key:value, not header
        if self.lines.len() == 1 && !contains_unquoted_colon(&first) {
            self.pos = 1;
            return Ok(infer_type(&first));
        }

        // Object
        self.parse_object(0)
    }

    // ── object ──────────────────────────────────────────────────────────

    fn parse_object(&mut self, base_indent: usize) -> Result<Value, String> {
        let mut map = Map::new();
        while !self.at_end() && self.indent() == base_indent {
            let line = self.content().to_string();

            // Array header: key[N]...
            if let Some(header) = try_parse_array_header(&line) {
                let key = header.key.clone();
                let val = self.parse_array_body(header, base_indent)?;
                map.insert(key, val);
                continue;
            }

            // key: value  or  key:
            let (key, rest) = split_kv(&line)?;
            self.pos += 1;

            if rest.is_empty() {
                let child = base_indent + self.indent_size;
                if !self.at_end() && self.indent() >= child {
                    map.insert(key, self.parse_object(child)?);
                } else {
                    map.insert(key, Value::Object(Map::new()));
                }
            } else {
                map.insert(key, infer_type(&rest));
            }
        }
        Ok(Value::Object(map))
    }

    // ── array ───────────────────────────────────────────────────────────

    fn parse_array_body(
        &mut self,
        header: ArrayHeader,
        base_indent: usize,
    ) -> Result<Value, String> {
        // Inline: key[N]: v1,v2,...
        if let Some(ref vals) = header.inline_values {
            let items = split_delimited(vals, header.delim);
            self.pos += 1;
            return Ok(Value::Array(items.iter().map(|v| infer_type(v)).collect()));
        }

        self.pos += 1; // consume header line
        let child = base_indent + self.indent_size;

        // Tabular: key[N]{f1,f2,...}:
        if let Some(ref fields) = header.fields {
            let mut arr = Vec::new();
            while !self.at_end()
                && self.indent() == child
                && !self.content().starts_with("- ")
                && self.content() != "-"
            {
                let row = self.content().to_string();
                let vals = split_delimited(&row, header.delim);
                let mut obj = Map::new();
                for (f, v) in fields.iter().zip(vals.iter()) {
                    obj.insert(f.clone(), infer_type(v));
                }
                arr.push(Value::Object(obj));
                self.pos += 1;
            }
            return Ok(Value::Array(arr));
        }

        // Expanded: key[N]:  with  - items
        let mut arr = Vec::new();
        while !self.at_end()
            && self.indent() == child
            && (self.content().starts_with("- ") || self.content() == "-")
        {
            arr.push(self.parse_list_item(child)?);
        }
        Ok(Value::Array(arr))
    }

    // ── list items ──────────────────────────────────────────────────────

    fn parse_list_item(&mut self, item_indent: usize) -> Result<Value, String> {
        let line = self.content().to_string();

        // Bare hyphen → empty object
        if line == "-" {
            self.pos += 1;
            return Ok(Value::Object(Map::new()));
        }

        let after = &line[2..]; // content after "- "

        // Array header on list item
        if let Some(header) = try_parse_array_header(after) {
            if header.key.is_empty() {
                return self.parse_list_item_bare_array(header, item_indent);
            } else {
                return self.parse_list_item_obj_array(header, item_indent);
            }
        }

        // Key-value → object item
        if let Some((key, rest)) = try_split_kv(after) {
            return self.parse_list_item_object(key, rest, item_indent);
        }

        // Primitive item
        self.pos += 1;
        Ok(infer_type(after))
    }

    /// `- [N]: ...` — bare array as list item
    fn parse_list_item_bare_array(
        &mut self,
        header: ArrayHeader,
        item_indent: usize,
    ) -> Result<Value, String> {
        if let Some(ref vals) = header.inline_values {
            let items = split_delimited(vals, header.delim);
            self.pos += 1;
            return Ok(Value::Array(items.iter().map(|v| infer_type(v)).collect()));
        }

        self.pos += 1;
        let child = item_indent + self.indent_size;

        if let Some(ref fields) = header.fields {
            let mut arr = Vec::new();
            while !self.at_end()
                && self.indent() == child
                && !self.content().starts_with("- ")
                && self.content() != "-"
            {
                let row = self.content().to_string();
                let vals = split_delimited(&row, header.delim);
                let mut obj = Map::new();
                for (f, v) in fields.iter().zip(vals.iter()) {
                    obj.insert(f.clone(), infer_type(v));
                }
                arr.push(Value::Object(obj));
                self.pos += 1;
            }
            return Ok(Value::Array(arr));
        }

        let mut arr = Vec::new();
        while !self.at_end()
            && self.indent() == child
            && (self.content().starts_with("- ") || self.content() == "-")
        {
            arr.push(self.parse_list_item(child)?);
        }
        Ok(Value::Array(arr))
    }

    /// `- key: value` / `- key:` → object list item
    fn parse_list_item_object(
        &mut self,
        key: String,
        rest: String,
        item_indent: usize,
    ) -> Result<Value, String> {
        self.pos += 1;
        let rest_indent = item_indent + self.indent_size;
        let mut map = Map::new();

        if rest.is_empty() {
            // First field is nested object or empty
            let nested = item_indent + 2 * self.indent_size;
            if !self.at_end() && self.indent() >= nested {
                map.insert(key, self.parse_object(nested)?);
            } else {
                map.insert(key, Value::Object(Map::new()));
            }
        } else {
            map.insert(key, infer_type(&rest));
        }

        self.parse_remaining_fields(&mut map, rest_indent)?;
        Ok(Value::Object(map))
    }

    /// `- key[N]...` → object with array as first field
    fn parse_list_item_obj_array(
        &mut self,
        header: ArrayHeader,
        item_indent: usize,
    ) -> Result<Value, String> {
        let key = header.key.clone();
        let rest_indent = item_indent + self.indent_size;
        let nested = item_indent + 2 * self.indent_size;

        let val = if let Some(ref vals) = header.inline_values {
            let items = split_delimited(vals, header.delim);
            self.pos += 1;
            Value::Array(items.iter().map(|v| infer_type(v)).collect())
        } else {
            self.pos += 1;
            if let Some(ref fields) = header.fields {
                // Tabular rows at nested indent
                let mut arr = Vec::new();
                while !self.at_end()
                    && self.indent() == nested
                    && !self.content().starts_with("- ")
                    && self.content() != "-"
                {
                    let row = self.content().to_string();
                    let vs = split_delimited(&row, header.delim);
                    let mut obj = Map::new();
                    for (f, v) in fields.iter().zip(vs.iter()) {
                        obj.insert(f.clone(), infer_type(v));
                    }
                    arr.push(Value::Object(obj));
                    self.pos += 1;
                }
                Value::Array(arr)
            } else {
                // Expanded items at nested indent
                let mut arr = Vec::new();
                while !self.at_end()
                    && self.indent() == nested
                    && (self.content().starts_with("- ") || self.content() == "-")
                {
                    arr.push(self.parse_list_item(nested)?);
                }
                Value::Array(arr)
            }
        };

        let mut map = Map::new();
        map.insert(key, val);
        self.parse_remaining_fields(&mut map, rest_indent)?;
        Ok(Value::Object(map))
    }

    /// Parse remaining object fields at a given indent level.
    fn parse_remaining_fields(
        &mut self,
        map: &mut Map<String, Value>,
        rest_indent: usize,
    ) -> Result<(), String> {
        while !self.at_end() && self.indent() == rest_indent {
            let line = self.content().to_string();

            if let Some(header) = try_parse_array_header(&line) {
                let k = header.key.clone();
                let v = self.parse_array_body(header, rest_indent)?;
                map.insert(k, v);
                continue;
            }

            let (k, r) = split_kv(&line)?;
            self.pos += 1;

            if r.is_empty() {
                let child = rest_indent + self.indent_size;
                if !self.at_end() && self.indent() >= child {
                    map.insert(k, self.parse_object(child)?);
                } else {
                    map.insert(k, Value::Object(Map::new()));
                }
            } else {
                map.insert(k, infer_type(&r));
            }
        }
        Ok(())
    }
}

// ── header parsing ──────────────────────────────────────────────────────────

fn try_parse_array_header(s: &str) -> Option<ArrayHeader> {
    let bracket_start = s.find('[')?;
    let bracket_end = s[bracket_start..].find(']')? + bracket_start;

    let raw_key = &s[..bracket_start];
    let key = if raw_key.is_empty() {
        String::new()
    } else {
        unquote_token(raw_key)
    };

    let bracket_inner = &s[bracket_start + 1..bracket_end];
    let (count, delim) = parse_bracket(bracket_inner)?;

    let rest = &s[bracket_end + 1..];
    let (fields, after) = if rest.starts_with('{') {
        let brace_end = rest.find('}')?;
        let fs: Vec<String> = split_delimited(&rest[1..brace_end], delim)
            .into_iter()
            .map(|f| unquote_token(&f))
            .collect();
        (Some(fs), &rest[brace_end + 1..])
    } else {
        (None, rest)
    };

    if !after.starts_with(':') {
        return None;
    }

    let after_colon = &after[1..];
    let inline_values = if after_colon.is_empty() {
        None
    } else if after_colon.starts_with(' ') {
        Some(after_colon[1..].to_string())
    } else {
        None
    };

    Some(ArrayHeader {
        key,
        count,
        delim,
        fields,
        inline_values,
    })
}

fn parse_bracket(s: &str) -> Option<(usize, Delim)> {
    if s.ends_with('\t') {
        Some((s[..s.len() - 1].parse().ok()?, Delim::Tab))
    } else if s.ends_with('|') {
        Some((s[..s.len() - 1].parse().ok()?, Delim::Pipe))
    } else {
        Some((s.parse().ok()?, Delim::Comma))
    }
}

// ── key-value parsing ───────────────────────────────────────────────────────

fn split_kv(s: &str) -> Result<(String, String), String> {
    try_split_kv(s).ok_or_else(|| format!("expected key: value, got: {}", s))
}

fn try_split_kv(s: &str) -> Option<(String, String)> {
    if s.starts_with('"') {
        let end = find_closing_quote(s, 1)?;
        let key = unescape(&s[1..end]);
        let rest = &s[end + 1..];
        if !rest.starts_with(':') {
            return None;
        }
        let val = if rest.len() > 1 && rest.as_bytes()[1] == b' ' {
            &rest[2..]
        } else {
            &rest[1..]
        };
        Some((key, val.to_string()))
    } else {
        let colon = s.find(':')?;
        let before = &s[..colon];
        if before.contains('[') {
            return None;
        }
        let val = if s.len() > colon + 1 && s.as_bytes()[colon + 1] == b' ' {
            &s[colon + 2..]
        } else {
            &s[colon + 1..]
        };
        Some((before.to_string(), val.to_string()))
    }
}

fn find_closing_quote(s: &str, start: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = start;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
        } else if b[i] == b'"' {
            return Some(i);
        } else {
            i += 1;
        }
    }
    None
}

fn contains_unquoted_colon(s: &str) -> bool {
    let mut in_q = false;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            esc = false;
            continue;
        }
        if c == '\\' && in_q {
            esc = true;
            continue;
        }
        if c == '"' {
            in_q = !in_q;
            continue;
        }
        if c == ':' && !in_q {
            return true;
        }
    }
    false
}

// ── value splitting & type inference ────────────────────────────────────────

fn split_delimited(s: &str, delim: Delim) -> Vec<String> {
    let dc = delim.char();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            cur.push(c);
            esc = false;
            continue;
        }
        if c == '\\' && in_q {
            cur.push(c);
            esc = true;
            continue;
        }
        if c == '"' {
            cur.push(c);
            in_q = !in_q;
            continue;
        }
        if c == dc && !in_q {
            out.push(cur);
            cur = String::new();
            continue;
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

fn infer_type(s: &str) -> Value {
    // Quoted string
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        return Value::String(unescape(&s[1..s.len() - 1]));
    }

    match s {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" => return Value::Null,
        _ => {}
    }

    if let Some(n) = try_parse_number(s) {
        return Value::Number(n);
    }

    Value::String(s.to_string())
}

fn try_parse_number(s: &str) -> Option<Number> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    // Leading zeros like "05" → not a number
    if b.len() > 1 && b[0] == b'0' && b[1].is_ascii_digit() {
        return None;
    }
    if b.len() > 2 && b[0] == b'-' && b[1] == b'0' && b[2].is_ascii_digit() {
        return None;
    }

    if let Ok(i) = s.parse::<i64>() {
        return Some(Number::from(i));
    }
    if let Ok(u) = s.parse::<u64>() {
        return Some(Number::from(u));
    }
    if let Ok(f) = s.parse::<f64>() {
        if f.is_finite() {
            return Number::from_f64(f);
        }
    }
    None
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut esc = false;
    for c in s.chars() {
        if esc {
            match c {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else {
            out.push(c);
        }
    }
    out
}

fn unquote_token(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        unescape(&s[1..s.len() - 1])
    } else {
        s.to_string()
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(toon: &str) -> Value {
        decode(toon).unwrap()
    }

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn empty_document() {
        assert_eq!(dec(""), json("{}"));
    }

    #[test]
    fn single_primitive() {
        assert_eq!(dec("42"), json("42"));
        assert_eq!(dec("hello"), json("\"hello\""));
        assert_eq!(dec("true"), json("true"));
        assert_eq!(dec("null"), json("null"));
    }

    #[test]
    fn simple_object() {
        assert_eq!(dec("x: 1\ny: 2"), json(r#"{"x":1,"y":2}"#));
    }

    #[test]
    fn nested_object() {
        assert_eq!(
            dec("a:\n  b:\n    c: 1"),
            json(r#"{"a":{"b":{"c":1}}}"#)
        );
    }

    #[test]
    fn empty_nested_object() {
        assert_eq!(dec("a:"), json(r#"{"a":{}}"#));
    }

    #[test]
    fn inline_array() {
        assert_eq!(dec("n[3]: 1,2,3"), json(r#"{"n":[1,2,3]}"#));
    }

    #[test]
    fn root_inline_array() {
        assert_eq!(dec("[3]: 1,2,3"), json("[1,2,3]"));
    }

    #[test]
    fn empty_array() {
        assert_eq!(dec("n[0]:"), json(r#"{"n":[]}"#));
    }

    #[test]
    fn tabular_array() {
        let toon = "users[2]{id,name}:\n  1,Alice\n  2,Bob";
        let expected = json(r#"{"users":[{"id":1,"name":"Alice"},{"id":2,"name":"Bob"}]}"#);
        assert_eq!(dec(toon), expected);
    }

    #[test]
    fn expanded_primitives() {
        let toon = "items[3]:\n  - 1\n  - hello\n  - true";
        assert_eq!(dec(toon), json(r#"{"items":[1,"hello",true]}"#));
    }

    #[test]
    fn expanded_objects() {
        let toon = "items[2]:\n  - id: 1\n    name: First\n  - id: 2\n    name: Second";
        let expected =
            json(r#"{"items":[{"id":1,"name":"First"},{"id":2,"name":"Second"}]}"#);
        assert_eq!(dec(toon), expected);
    }

    #[test]
    fn array_of_arrays() {
        let toon = "pairs[2]:\n  - [2]: 1,2\n  - [2]: 3,4";
        assert_eq!(dec(toon), json(r#"{"pairs":[[1,2],[3,4]]}"#));
    }

    #[test]
    fn string_quoting() {
        assert_eq!(dec(r#"x: "true""#), json(r#"{"x":"true"}"#));
        assert_eq!(dec(r#"x: "42""#), json(r#"{"x":"42"}"#));
        assert_eq!(dec(r#"x: """#), json(r#"{"x":""}"#));
    }

    #[test]
    fn quoted_key() {
        assert_eq!(dec(r#""has space": 1"#), json(r#"{"has space":1}"#));
    }

    #[test]
    fn spec_roundtrip() {
        let toon = "\
context:
  task: Our favorite hikes together
  location: Boulder
  season: spring_2025
friends[3]: ana,luis,sam
hikes[3]{id,name,distanceKm,elevationGain,companion,wasSunny}:
  1,Blue Lake Trail,7.5,320,ana,true
  2,Ridge Overlook,9.2,540,luis,false
  3,Wildflower Loop,5.1,180,sam,true";

        let expected = json(
            r#"{
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
        }"#,
        );
        assert_eq!(dec(toon), expected);
    }

    #[test]
    fn mixed_expanded_list() {
        let toon = "items[3]:\n  - 1\n  - a: 1\n  - text";
        assert_eq!(
            dec(toon),
            json(r#"{"items":[1,{"a":1},"text"]}"#)
        );
    }

    #[test]
    fn object_with_nested_array_in_list() {
        let toon = "data[1]:\n  - tags[2]: x,y\n    name: test";
        let expected = json(r#"{"data":[{"tags":["x","y"],"name":"test"}]}"#);
        assert_eq!(dec(toon), expected);
    }

    #[test]
    fn escape_sequences() {
        assert_eq!(
            dec(r#"x: "hello\nworld""#),
            json(r#"{"x":"hello\nworld"}"#)
        );
    }
}
