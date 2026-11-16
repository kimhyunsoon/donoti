// donoti 전용 JS 의미 헬퍼 (core/js.rs 에 없는 것). 크롤러가 외부 응답·config JSON 을 TS 처럼 다룬다.
// JS 값은 Option<&Value> 로 표현한다 (None = undefined).
use serde_json::Value;

use crate::core::js;

/// `o.k` - o 가 undefined/null 이면 TypeError, 원시값이면 undefined
pub fn prop<'a>(o: Option<&'a Value>, k: &str) -> Result<Option<&'a Value>, String> {
    match o {
        None => Err(format!(
            "Cannot read properties of undefined (reading '{k}')"
        )),
        Some(Value::Null) => Err(format!("Cannot read properties of null (reading '{k}')")),
        Some(Value::Object(m)) => Ok(m.get(k)),
        Some(_) => Ok(None),
    }
}

/// JS 문자열 비교 a > b (UTF-16 코드 단위 순)
pub fn js_str_gt(a: &str, b: &str) -> bool {
    a.encode_utf16().cmp(b.encode_utf16()) == std::cmp::Ordering::Greater
}

/// JS 문자열 비교 (UTF-16 코드 단위 순)
pub fn js_str_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// 일반 객체에서 상속되는 Object.prototype 속성 이름 (RECORD[이 이름] 이 undefined 가 아니다)
pub const OBJECT_PROTO_KEYS: &[&str] = &[
    "__proto__",
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "toLocaleString",
];

pub fn is_proto_key(k: &str) -> bool {
    OBJECT_PROTO_KEYS.contains(&k)
}

// JS 의 \s (u 플래그 포함): WhiteSpace + LineTerminator
pub const JS_S: &str = r"[\t\n\x0B\x0C\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]";
// JS 의 \S
pub const JS_NS: &str = r"[^\t\n\x0B\x0C\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]";

/// 공유 텍스트에서 URL 뽑기: input.match(/https?:\/\/\S+/)?.[0] ?? ''
pub fn extract_url(input: &str) -> String {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(&format!("https?://{JS_NS}+")).unwrap());
    RE.find(input)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

/// resolveXxxLink(req.body.url ?? '') 의 인자 - 문자열이 아니면 input.match 에서 TypeError
pub fn resolve_input(body: Option<&Value>) -> Result<String, String> {
    let url = nn(prop(body, "url")?);
    match url {
        None => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err("input.match is not a function".into()),
    }
}

/// 객체 리터럴 - undefined(None) 인 키는 JSON 에서 빠진다
pub fn obj(fields: Vec<(&str, Option<Value>)>) -> Value {
    let mut m = serde_json::Map::new();
    for (k, v) in fields {
        if let Some(v) = v {
            m.insert(k.to_string(), v);
        }
    }
    Value::Object(m)
}

/// SameValueZero (Map 키·includes)
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => js::as_f64(x) == js::as_f64(y),
        // 객체·배열은 참조 비교 - 서로 다른 JSON 값이면 같을 수 없다
        (Value::Object(_), _) | (Value::Array(_), _) => false,
        _ => a == b,
    }
}

/// `v.toLocaleString('ko-KR')` - 숫자는 ko-KR 표기, 문자열은 그대로
pub fn locale_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::Number(n)) => to_locale_ko(js::as_f64(n)),
        other => js::to_js_string(other),
    }
}

/// `a !== b` (객체·배열은 서로 다른 참조로 본다)
pub fn strict_ne(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(Value::Number(x)), Some(Value::Number(y))) => js::as_f64(x) != js::as_f64(y),
        (Some(Value::Object(_)), _) | (Some(Value::Array(_)), _) => true,
        (Some(x), Some(y)) => x != y,
        _ => true,
    }
}

/// `a < b` (JS 관계 연산: 둘 다 문자열이면 문자열 비교, 아니면 숫자로)
pub fn js_lt(a: Option<&Value>, b: Option<&Value>) -> bool {
    if let (Some(Value::String(x)), Some(Value::String(y))) = (a, b) {
        return js_str_cmp(x, y) == std::cmp::Ordering::Less;
    }
    js::to_number(a) < js::to_number(b)
}

/// `${n.toLocaleString('ko-KR')}원` - n 이 undefined/null 이면 TypeError
pub fn won(v: Option<&Value>) -> Result<String, String> {
    match v {
        None => Err("Cannot read properties of undefined (reading 'toLocaleString')".into()),
        Some(Value::Null) => {
            Err("Cannot read properties of null (reading 'toLocaleString')".into())
        }
        v => Ok(format!("{}원", locale_of(v))),
    }
}

/// 문자열의 UTF-16 단위 위치 → 바이트 위치 변환용: s[..byte] 의 UTF-16 길이
pub fn u16_index(s: &str, byte: usize) -> usize {
    js::utf16_len(&s[..byte])
}

/// JS 관계 비교 결과 (둘 다 문자열이면 문자열 비교, 아니면 숫자로. NaN 이면 None)
pub fn js_cmp(a: Option<&Value>, b: Option<&Value>) -> Option<std::cmp::Ordering> {
    if let (Some(Value::String(x)), Some(Value::String(y))) = (a, b) {
        return Some(js_str_cmp(x, y));
    }
    js::to_number(a).partial_cmp(&js::to_number(b))
}

/// `a >= b`
pub fn js_ge(a: Option<&Value>, b: Option<&Value>) -> bool {
    matches!(
        js_cmp(a, b),
        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
    )
}

/// `a <= b`
pub fn js_le(a: Option<&Value>, b: Option<&Value>) -> bool {
    matches!(
        js_cmp(a, b),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    )
}

/// `a === b` (객체·배열은 서로 다른 참조로 본다)
pub fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    !strict_ne(a, b)
}

/// `s.includes(x)` (문자열일 때만, 아니면 TypeError)
pub fn str_includes(v: Option<&Value>, x: &str, expr: &str) -> Result<bool, String> {
    match v {
        Some(Value::String(s)) => Ok(s.contains(x)),
        Some(Value::Array(a)) => Ok(a.iter().any(|e| e.as_str() == Some(x))),
        None => Err("Cannot read properties of undefined (reading 'includes')".into()),
        Some(Value::Null) => Err("Cannot read properties of null (reading 'includes')".into()),
        Some(_) => Err(format!("{expr}.includes is not a function")),
    }
}

/// `s.slice(a, b)` (문자열 아니면 TypeError)
pub fn str_slice(v: Option<&Value>, a: usize, b: usize, expr: &str) -> Result<String, String> {
    match v {
        Some(Value::String(s)) => Ok(js::utf16_slice(s, a, b)),
        Some(Value::Array(_)) => Ok(String::new()),
        None => Err("Cannot read properties of undefined (reading 'slice')".into()),
        Some(Value::Null) => Err("Cannot read properties of null (reading 'slice')".into()),
        Some(_) => Err(format!("{expr}.slice is not a function")),
    }
}

/// `o?.k`
pub fn opt<'a>(o: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    match o {
        Some(Value::Object(m)) => m.get(k),
        _ => None,
    }
}

/// `v ?? null` 처럼 null 을 undefined 와 같이 본다
pub fn nn(v: Option<&Value>) -> Option<&Value> {
    match v {
        Some(Value::Null) | None => None,
        x => x,
    }
}

pub fn str_of(v: Option<&Value>) -> Option<&str> {
    v.and_then(|v| v.as_str())
}

/// JS `String(n)` / 템플릿 리터럴의 숫자 (NaN·Infinity 포함)
pub fn num_str(f: f64) -> String {
    if f.is_nan() {
        "NaN".into()
    } else if f.is_infinite() {
        if f > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        js::number_to_string(f)
    }
}

/// 템플릿 리터럴 `${v}` (undefined → 'undefined')
pub fn tpl(v: Option<&Value>) -> String {
    js::to_js_string(v)
}

/// JS 숫자로 읽기 (typeof v === 'number' 일 때만)
pub fn as_num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => Some(js::as_f64(n)),
        _ => None,
    }
}

/// JS `Math.round` (x.5 는 +∞ 쪽)
pub fn math_round(f: f64) -> f64 {
    if !f.is_finite() {
        return f;
    }
    let fl = f.floor();
    if f - fl >= 0.5 { fl + 1.0 } else { fl }
}

/// `n.toLocaleString('ko-KR')` - 쉼표 묶음, 소수 최대 3자리(최단 십진 표기 기준 반올림 halfExpand)
pub fn to_locale_ko(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "∞" } else { "-∞" }.into();
    }
    let neg = f.is_sign_negative();
    // 최단 십진 표기의 숫자열과 소수점 위치
    let s = js::number_to_string(f.abs());
    let (digits, point) = decimal_digits(&s);
    // 소수 3자리까지 반올림
    let keep = (point + 3).max(0) as usize;
    let mut d: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    let mut point = point;
    if d.len() > keep {
        let round_up = d[keep] >= 5;
        d.truncate(keep);
        if round_up {
            let mut i = d.len();
            loop {
                if i == 0 {
                    d.insert(0, 1);
                    point += 1;
                    break;
                }
                i -= 1;
                if d[i] == 9 {
                    d[i] = 0;
                } else {
                    d[i] += 1;
                    break;
                }
            }
        }
    }
    // 정수부·소수부
    let p = point.max(0) as usize;
    while d.len() < p {
        d.push(0);
    }
    let (int_d, frac_d): (Vec<u8>, Vec<u8>) = if point >= 0 {
        (d[..p].to_vec(), d[p..].to_vec())
    } else {
        let mut f = vec![0u8; (-point) as usize];
        f.extend_from_slice(&d);
        (vec![], f)
    };
    let int_s: String = if int_d.is_empty() {
        "0".into()
    } else {
        int_d.iter().map(|x| (b'0' + x) as char).collect()
    };
    let int_s = int_s.trim_start_matches('0');
    let int_s = if int_s.is_empty() { "0" } else { int_s };
    let mut frac_s: String = frac_d.iter().map(|x| (b'0' + x) as char).collect();
    while frac_s.ends_with('0') {
        frac_s.pop();
    }
    let mut grouped = String::new();
    for (i, c) in int_s.chars().enumerate() {
        if i > 0 && (int_s.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let body = if frac_s.is_empty() {
        grouped
    } else {
        format!("{grouped}.{frac_s}")
    };
    if neg { format!("-{body}") } else { body }
}

/// JS 숫자 표기('123.45', '1e+21', '1.5e-7') → (유효 숫자열, 소수점 위치)
fn decimal_digits(s: &str) -> (String, i32) {
    let (mant, exp) = match s.find('e') {
        Some(i) => (&s[..i], s[i + 1..].parse::<i32>().unwrap_or(0)),
        None => (s, 0),
    };
    let (ip, fp) = match mant.find('.') {
        Some(i) => (&mant[..i], &mant[i + 1..]),
        None => (mant, ""),
    };
    (format!("{ip}{fp}"), ip.len() as i32 + exp)
}

// ───────── JSON.parse (V8 오류 문구) ─────────

struct Scan<'a> {
    s: &'a [u16],
    i: usize,
}

enum JErr {
    End,
    Token(usize),
    Msg(&'static str, usize),
}

impl Scan<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], 0x20 | 0x09 | 0x0a | 0x0d) {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u16> {
        self.s.get(self.i).copied()
    }
    fn value(&mut self) -> Result<(), JErr> {
        self.ws();
        let Some(c) = self.peek() else {
            return Err(JErr::End);
        };
        match c {
            0x7b => self.object(),
            0x5b => self.array(),
            0x22 => self.string(),
            0x2d | 0x30..=0x39 => self.number(),
            0x74 => self.literal("true"),
            0x66 => self.literal("false"),
            0x6e => self.literal("null"),
            _ => Err(JErr::Token(self.i)),
        }
    }
    fn literal(&mut self, w: &str) -> Result<(), JErr> {
        for b in w.encode_utf16() {
            match self.peek() {
                None => return Err(JErr::End),
                Some(c) if c == b => self.i += 1,
                Some(_) => return Err(JErr::Token(self.i)),
            }
        }
        Ok(())
    }
    fn digits(&mut self) -> usize {
        let st = self.i;
        while matches!(self.peek(), Some(0x30..=0x39)) {
            self.i += 1;
        }
        self.i - st
    }
    fn number(&mut self) -> Result<(), JErr> {
        if self.peek() == Some(0x2d) {
            self.i += 1;
            if !matches!(self.peek(), Some(0x30..=0x39)) {
                return Err(JErr::Msg("No number after minus sign in JSON", self.i));
            }
        }
        if self.peek() == Some(0x30) {
            self.i += 1;
        } else {
            self.digits();
        }
        if self.peek() == Some(0x2e) {
            self.i += 1;
            if self.digits() == 0 {
                return Err(JErr::Msg("Unterminated fractional number in JSON", self.i));
            }
        }
        if matches!(self.peek(), Some(0x65 | 0x45)) {
            self.i += 1;
            if matches!(self.peek(), Some(0x2b | 0x2d)) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return Err(JErr::Msg(
                    "Exponent part is missing a number in JSON",
                    self.i,
                ));
            }
        }
        Ok(())
    }
    fn string(&mut self) -> Result<(), JErr> {
        self.i += 1;
        loop {
            let Some(c) = self.peek() else {
                return Err(JErr::Msg("Unterminated string in JSON", self.i));
            };
            match c {
                0x22 => {
                    self.i += 1;
                    return Ok(());
                }
                0x5c => {
                    self.i += 1;
                    match self.peek() {
                        None => return Err(JErr::End),
                        Some(0x22 | 0x5c | 0x2f | 0x62 | 0x66 | 0x6e | 0x72 | 0x74) => self.i += 1,
                        Some(0x75) => {
                            self.i += 1;
                            for _ in 0..4 {
                                match self.peek() {
                                    Some(h)
                                        if (h as u8 as u16 == h)
                                            && (h as u8).is_ascii_hexdigit() =>
                                    {
                                        self.i += 1
                                    }
                                    _ => {
                                        return Err(JErr::Msg(
                                            "Bad Unicode escape in JSON",
                                            self.i,
                                        ));
                                    }
                                }
                            }
                        }
                        Some(_) => return Err(JErr::Msg("Bad escaped character in JSON", self.i)),
                    }
                }
                0..=0x1f => {
                    return Err(JErr::Msg(
                        "Bad control character in string literal in JSON",
                        self.i,
                    ));
                }
                _ => self.i += 1,
            }
        }
    }
    fn array(&mut self) -> Result<(), JErr> {
        self.i += 1;
        self.ws();
        if self.peek() == Some(0x5d) {
            self.i += 1;
            return Ok(());
        }
        loop {
            self.value()?;
            self.ws();
            match self.peek() {
                Some(0x2c) => self.i += 1,
                Some(0x5d) => {
                    self.i += 1;
                    return Ok(());
                }
                _ => {
                    return Err(JErr::Msg(
                        "Expected ',' or ']' after array element in JSON",
                        self.i,
                    ));
                }
            }
        }
    }
    fn object(&mut self) -> Result<(), JErr> {
        self.i += 1;
        self.ws();
        match self.peek() {
            Some(0x7d) => {
                self.i += 1;
                return Ok(());
            }
            Some(0x22) => {}
            _ => return Err(JErr::Msg("Expected property name or '}' in JSON", self.i)),
        }
        loop {
            self.string()?;
            self.ws();
            match self.peek() {
                Some(0x3a) => self.i += 1,
                _ => {
                    return Err(JErr::Msg(
                        "Expected ':' after property name in JSON",
                        self.i,
                    ));
                }
            }
            self.value()?;
            self.ws();
            match self.peek() {
                Some(0x2c) => {
                    self.i += 1;
                    self.ws();
                    if self.peek() != Some(0x22) {
                        return Err(JErr::Msg(
                            "Expected double-quoted property name in JSON",
                            self.i,
                        ));
                    }
                }
                Some(0x7d) => {
                    self.i += 1;
                    return Ok(());
                }
                _ => {
                    return Err(JErr::Msg(
                        "Expected ',' or '}' after property value in JSON",
                        self.i,
                    ));
                }
            }
        }
    }
}

fn line_col(s: &[u16], pos: usize) -> (usize, usize) {
    let before = &s[..pos.min(s.len())];
    let line = before.iter().filter(|&&c| c == 0x0a).count() + 1;
    let col = match before.iter().rposition(|&c| c == 0x0a) {
        Some(nl) => pos - nl,
        None => pos + 1,
    };
    (line, col)
}

/// JSON.parse(text) - 실패하면 V8(Node 22) 과 같은 SyntaxError 문구
pub fn json_parse(text: &str) -> Result<Value, String> {
    let u: Vec<u16> = text.encode_utf16().collect();
    let mut sc = Scan { s: &u, i: 0 };
    let r = sc.value().and_then(|_| {
        sc.ws();
        if sc.i < u.len() {
            Err(JErr::Msg(
                "Unexpected non-whitespace character after JSON",
                sc.i,
            ))
        } else {
            Ok(())
        }
    });
    match r {
        Ok(()) => serde_json::from_str(text).map_err(|e| e.to_string()),
        Err(JErr::End) => Err("Unexpected end of JSON input".into()),
        Err(JErr::Msg(m, pos)) => {
            let (l, c) = line_col(&u, pos);
            Err(format!("{m} at position {pos} (line {l} column {c})"))
        }
        Err(JErr::Token(pos)) => {
            let tok = String::from_utf16_lossy(&u[pos..pos + 1]);
            let ctx = if u.len() <= 20 {
                format!("\"{text}\"")
            } else {
                let st = pos.saturating_sub(10);
                let en = (pos + 10).min(u.len());
                format!(
                    "{}\"{}\"{}",
                    if st > 0 { "..." } else { "" },
                    String::from_utf16_lossy(&u[st..en]),
                    if en < u.len() { "..." } else { "" }
                )
            };
            Err(format!("Unexpected token '{tok}', {ctx} is not valid JSON"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_ko() {
        for (f, s) in [
            (1234567.891, "1,234,567.891"),
            (0.0005, "0.001"),
            (1.0005, "1.001"),
            (-1234.0, "-1,234"),
            (1e21, "1,000,000,000,000,000,000,000"),
            (2.5, "2.5"),
            (1.2345, "1.235"),
            (1.0015, "1.002"),
            (-0.0, "-0"),
            (47000.0, "47,000"),
            (999.9999, "1,000"),
            (0.0, "0"),
            (100.0, "100"),
        ] {
            assert_eq!(to_locale_ko(f), s, "{f}");
        }
        assert_eq!(to_locale_ko(f64::NAN), "NaN");
    }

    #[test]
    fn v8_json_errors() {
        let cases = [
            ("", "Unexpected end of JSON input"),
            (
                "<!DOCTYPE html><html><head><title>x</title></head></html>",
                "Unexpected token '<', \"<!DOCTYPE \"... is not valid JSON",
            ),
            (
                "<html>",
                "Unexpected token '<', \"<html>\" is not valid JSON",
            ),
            ("  <b", "Unexpected token '<', \"  <b\" is not valid JSON"),
            (
                "{\"a\":1}x",
                "Unexpected non-whitespace character after JSON at position 7 (line 1 column 8)",
            ),
            ("{\"a\":", "Unexpected end of JSON input"),
            (
                "{a:1}",
                "Expected property name or '}' in JSON at position 1 (line 1 column 2)",
            ),
            ("abc", "Unexpected token 'a', \"abc\" is not valid JSON"),
            (
                "{\"a\":1,}",
                "Expected double-quoted property name in JSON at position 7 (line 1 column 8)",
            ),
            (
                "[1,2",
                "Expected ',' or ']' after array element in JSON at position 4 (line 1 column 5)",
            ),
            ("nul", "Unexpected end of JSON input"),
            (
                "\"abc",
                "Unterminated string in JSON at position 4 (line 1 column 5)",
            ),
            (
                "<xxxxxxxxxxxxxxxxxxx",
                "Unexpected token '<', \"<xxxxxxxxxxxxxxxxxxx\" is not valid JSON",
            ),
            (
                "<xxxxxxxxxxxxxxxxxxxx",
                "Unexpected token '<', \"<xxxxxxxxx\"... is not valid JSON",
            ),
            (
                "{\"aaaaaaaaaaaaaaaa\":<bbbbbbbbbbbbbbbbbbbbbbbb}",
                "Unexpected token '<', ...\"aaaaaaaa\":<bbbbbbbbb\"... is not valid JSON",
            ),
            (
                "{\"a\":1,\"b\":tru}",
                "Unexpected token '}', \"{\"a\":1,\"b\":tru}\" is not valid JSON",
            ),
            (
                "[1,2,3,4,5,6,7,8,9,10,11,12,13,<]",
                "Unexpected token '<', ...\",11,12,13,<]\" is not valid JSON",
            ),
            (
                "{\"a\":1",
                "Expected ',' or '}' after property value in JSON at position 6 (line 1 column 7)",
            ),
            (
                "{\"a\"",
                "Expected ':' after property name in JSON at position 4 (line 1 column 5)",
            ),
            (
                "{",
                "Expected property name or '}' in JSON at position 1 (line 1 column 2)",
            ),
            ("[", "Unexpected end of JSON input"),
            (
                "{\"a\":1,",
                "Expected double-quoted property name in JSON at position 7 (line 1 column 8)",
            ),
            ("[1,", "Unexpected end of JSON input"),
            (
                "-",
                "No number after minus sign in JSON at position 1 (line 1 column 2)",
            ),
            (
                "1.",
                "Unterminated fractional number in JSON at position 2 (line 1 column 3)",
            ),
            (
                "1e",
                "Exponent part is missing a number in JSON at position 2 (line 1 column 3)",
            ),
            ("tru", "Unexpected end of JSON input"),
            (
                "[1 ",
                "Expected ',' or ']' after array element in JSON at position 3 (line 1 column 4)",
            ),
            (
                "\"\\u12",
                "Bad Unicode escape in JSON at position 5 (line 1 column 6)",
            ),
            ("\"\\", "Unexpected end of JSON input"),
        ];
        for (s, m) in cases {
            assert_eq!(json_parse(s).unwrap_err(), m, "{s}");
        }
        assert_eq!(
            json_parse(" {\"a\":[1,2.5e3,true,null,\"x\\u00e9\"]} ").unwrap()["a"][1],
            2500.0
        );
    }

    #[test]
    fn round() {
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(-2.5), -2.0);
        assert_eq!(math_round(0.49999999999999994), 0.0);
        assert_eq!(math_round(46999.6), 47000.0);
    }
}
