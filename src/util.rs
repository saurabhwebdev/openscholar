#![allow(clippy::manual_is_multiple_of)] // is_multiple_of not stable; keep compatibility

/// Format an amount in minor units (cents) as a human string: `123456` -> `"1,234.56"`.
pub fn format_cents(cents: i64) -> String {
    let negative = cents < 0;
    let abs = cents.unsigned_abs();
    let major = abs / 100;
    let minor = abs % 100;
    format!(
        "{}{}.{:02}",
        if negative { "-" } else { "" },
        group_thousands(major),
        minor
    )
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Minimal percent-encoding for query values. Keeps `/` readable for `next=` paths,
/// encodes spaces as `+`.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Minimal standard base64 decoder (no external deps).
pub fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let mut s = input.trim().to_string();
    if let Some(idx) = s.rfind(',') {
        // strip data:...;base64, prefix if present
        if s[..idx].contains("base64") {
            s = s[idx + 1..].to_string();
        }
    }
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    if s.len() % 4 != 0 {
        return Err("invalid base64 length".to_string());
    }
    fn val(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            b'=' => Ok(0),
            _ => Err(format!("invalid base64 char: {}", c as char)),
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let a = val(chunk[0])?;
        let b = val(chunk[1])?;
        let c = val(chunk[2])?;
        let d = val(chunk[3])?;
        let n = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | (d as u32);
        out.push((n >> 16) as u8);
        if chunk[2] != b'=' {
            out.push((n >> 8) as u8);
        }
        if chunk[3] != b'=' {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_zero_and_small_amounts() {
        assert_eq!(format_cents(0), "0.00");
        assert_eq!(format_cents(5), "0.05");
        assert_eq!(format_cents(99), "0.99");
        assert_eq!(format_cents(100), "1.00");
    }

    #[test]
    fn groups_thousands() {
        assert_eq!(format_cents(123_456), "1,234.56");
        assert_eq!(format_cents(100_000_000), "1,000,000.00");
    }

    #[test]
    fn formats_negatives() {
        assert_eq!(format_cents(-250), "-2.50");
        assert_eq!(format_cents(-1), "-0.01");
    }

    #[test]
    fn decodes_base64_basic() {
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("").unwrap(), b"");
        assert!(decode_base64("!!!!").is_err());
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(
            url_encode("Invalid email or password"),
            "Invalid+email+or+password"
        );
        assert_eq!(url_encode("a&b=c?d"), "a%26b%3Dc%3Fd");
        assert_eq!(url_encode("/students?page=1"), "/students%3Fpage%3D1");
        assert_eq!(url_encode("ok-_.~AZ09"), "ok-_.~AZ09");
    }
}
