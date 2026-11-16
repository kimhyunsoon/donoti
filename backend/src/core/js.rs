// 4개 앱(dogroo·donoti·docook·doquiz) 공통 사본 - 고칠 때 네 곳을 같이 고친다.
//
// JS 의 JSON.stringify 와 같은 출력. serde_json 과 다른 점:
// - 숫자: 정수값인 실수는 소수점 없이(2.0 → 2), 지수 표기 경계(1e21, 1e-7)와 `e+` 표기
// - 객체 키: 배열 인덱스 꼴 키(0..2^32-2)가 먼저 숫자 오름차순, 나머지는 삽입 순
use serde_json::{Map, Number, Value};

/// JS `Number#toString()` 과 같은 표기
pub fn number_to_string(f: f64) -> String {
    if f.is_nan() || f.is_infinite() {
        return "null".into(); // JSON.stringify 에서만 쓰므로 null
    }
    if f == 0.0 {
        return "0".into();
    }
    let mut buf = ryu::Buffer::new();
    let s = buf.format_finite(f.abs());
    // ryu 출력에서 유효 숫자(digits)와 십진 지수(n: 소수점 위치)를 뽑는다
    let (mantissa, exp) = match s.find('e') {
        Some(i) => (&s[..i], s[i + 1..].parse::<i32>().unwrap()),
        None => (s, 0),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };
    let mut digits: String = format!("{int_part}{frac_part}");
    let mut n = int_part.len() as i32 + exp;
    // 앞의 0 제거
    let lead = digits.len() - digits.trim_start_matches('0').len();
    digits.drain(..lead);
    n -= lead as i32;
    let digits = digits.trim_end_matches('0').to_string();
    let k = digits.len() as i32;
    let sign = if f < 0.0 { "-" } else { "" };
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let es = if e >= 0 {
            format!("+{e}")
        } else {
            e.to_string()
        };
        if k == 1 {
            format!("{digits}e{es}")
        } else {
            format!("{}.{}e{es}", &digits[..1], &digits[1..])
        }
    };
    format!("{sign}{body}")
}

/// serde_json Number → f64 (JS 는 모든 숫자가 double)
pub fn as_f64(n: &Number) -> f64 {
    n.as_f64().unwrap_or(f64::NAN)
}

/// f64 → JSON 값. 정수값이면 정수 Number 로 둬서 serde 쪽에서도 2 로 보이게
pub fn num(f: f64) -> Value {
    if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
        Value::from(f as i64)
    } else {
        Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

fn is_array_index(k: &str) -> bool {
    if k.is_empty() || k.len() > 10 || !k.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if k.len() > 1 && k.starts_with('0') {
        return false;
    }
    k.parse::<u64>().map(|n| n < 4_294_967_295).unwrap_or(false)
}

fn write_str(out: &mut String, s: &str) {
    out.push_str(&serde_json::to_string(s).unwrap());
}

fn write_value(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64()
                && i.unsigned_abs() <= 9_007_199_254_740_992
            {
                out.push_str(&i.to_string());
                return;
            }
            out.push_str(&number_to_string(as_f64(n)));
        }
        Value::String(s) => write_str(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, x);
            }
            out.push(']');
        }
        Value::Object(m) => write_object(out, m),
    }
}

fn write_object(out: &mut String, m: &Map<String, Value>) {
    out.push('{');
    let mut first = true;
    let mut index_keys: Vec<(&String, &Value)> =
        m.iter().filter(|(k, _)| is_array_index(k)).collect();
    index_keys.sort_by_key(|(k, _)| k.parse::<u64>().unwrap());
    let rest = m.iter().filter(|(k, _)| !is_array_index(k));
    for (k, v) in index_keys.into_iter().chain(rest) {
        if !first {
            out.push(',');
        }
        first = false;
        write_str(out, k);
        out.push(':');
        write_value(out, v);
    }
    out.push('}');
}

/// JSON.stringify(v)
pub fn stringify(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v);
    out
}

/// JS 문자열 길이 (UTF-16 코드 단위)
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// JS `s.slice(0, n)` (UTF-16 단위, 서로게이트 반쪽은 U+FFFD)
pub fn utf16_slice(s: &str, start: usize, end: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    let end = end.min(u.len());
    let start = start.min(end);
    String::from_utf16_lossy(&u[start..end])
}

/// JS truthy (값이 있는 경우만; undefined 는 None 으로 표현)
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => {
            let f = as_f64(n);
            f != 0.0 && !f.is_nan()
        }
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// JS `String(v)`
pub fn to_js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => number_to_string(as_f64(n)),
        },
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                Value::Null => String::new(),
                _ => to_js_string(Some(x)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// JS `Number.isInteger(v)`
pub fn is_integer(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Number(n)) if { let f = as_f64(n); f.is_finite() && f.fract() == 0.0 })
}

/// JS 공백 (String.prototype.trim 이 지우는 문자: WhiteSpace + LineTerminator, BOM 포함)
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// JS `s.trim()`
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// JS `Number(s)` (문자열)
pub fn string_to_number(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let radix = |p: &str, r: u32| -> Option<f64> {
        let digits = t
            .strip_prefix(p)
            .or_else(|| t.strip_prefix(&p.to_uppercase()))?;
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(r)) {
            return Some(f64::NAN);
        }
        Some(
            digits
                .chars()
                .fold(0.0, |acc, c| acc * r as f64 + c.to_digit(r).unwrap() as f64),
        )
    };
    if let Some(v) = radix("0x", 16)
        .or_else(|| radix("0o", 8))
        .or_else(|| radix("0b", 2))
    {
        return v;
    }
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - f;
    }
    if int_digits + frac_digits == 0 {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return f64::NAN;
        }
    }
    if i != b.len() {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// JS `Number(v)`
pub fn to_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(n)) => as_f64(n),
        Some(Value::String(s)) => string_to_number(s),
        Some(Value::Array(_)) | Some(Value::Object(_)) => string_to_number(&to_js_string(v)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_like_js() {
        let cases = [
            (2.0, "2"),
            (0.1, "0.1"),
            (1.5, "1.5"),
            (-0.0, "0"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (123.456, "123.456"),
            (1.2345e-10, "1.2345e-10"),
            (5e300, "5e+300"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-3.25, "-3.25"),
            (100.0, "100"),
        ];
        for (f, s) in cases {
            assert_eq!(number_to_string(f), s, "{f}");
        }
    }

    #[test]
    fn key_order_like_js() {
        let v: Value = serde_json::from_str(r#"{"b":1,"2":2,"a":3,"1":4,"01":5,"x":2.0}"#).unwrap();
        assert_eq!(stringify(&v), r#"{"1":4,"2":2,"b":1,"a":3,"01":5,"x":2}"#);
    }

    #[test]
    fn number_parse_like_js() {
        for (s, n) in [
            ("12", 12.0),
            (" 12 ", 12.0),
            ("", 0.0),
            ("0x10", 16.0),
            ("1e2", 100.0),
            ("12.0", 12.0),
            (".5", 0.5),
            ("5.", 5.0),
            ("-3", -3.0),
        ] {
            assert_eq!(string_to_number(s), n, "{s}");
        }
        for s in ["abc", "1,2", "inf", "nan", "infinity", "-0x10", "1e", "."] {
            assert!(string_to_number(s).is_nan(), "{s}");
        }
    }

    #[test]
    fn utf16() {
        assert_eq!(utf16_len("a😀"), 3);
        assert_eq!(utf16_slice("가나다", 0, 2), "가나");
    }
}
