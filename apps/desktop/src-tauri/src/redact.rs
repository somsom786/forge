//! Best-effort secret redaction applied before anything is persisted or logged.
//! This reduces accidental leakage; it is not a guarantee.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

struct Patterns {
    plain: Vec<Regex>,
    assignment: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        plain: [
            r"sk-[A-Za-z0-9_\-]{20,}",
            r"gh[pousr]_[A-Za-z0-9]{20,}",
            r"github_pat_[A-Za-z0-9_]{30,}",
            r"AKIA[0-9A-Z]{16}",
            r"xox[abprs]-[A-Za-z0-9\-]{10,}",
            r"(?i)bearer\s+[A-Za-z0-9._\-]{20,}",
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            // A command may crash or be truncated before printing the footer; never expose the partial key tail.
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*$",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("valid redaction regex"))
        .collect(),
        assignment: Regex::new(
            r#"(?i)(api[_-]?key|secret|token|passwd|password)(["']?\s*[=:]\s*["']?)[^\s"',;]{6,}"#,
        )
        .expect("valid assignment regex"),
    })
}

pub fn redact_str(s: &str) -> String {
    let p = patterns();
    let mut out = s.to_string();
    for re in &p.plain {
        out = re.replace_all(&out, "[REDACTED]").into_owned();
    }
    p.assignment.replace_all(&out, "$1$2[REDACTED]").into_owned()
}

pub fn redact_value(v: &mut Value) {
    match v {
        Value::String(s) => *s = redact_str(s),
        Value::Array(a) => a.iter_mut().for_each(redact_value),
        Value::Object(m) => m.values_mut().for_each(redact_value),
        _ => {}
    }
}

/// Truncate to at most `max` bytes on a char boundary, noting the cut.
pub fn truncate(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_string(), false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}… [truncated {} bytes]", &s[..end], s.len() - end), true)
}

/// The last `max` bytes of `s` (on a char boundary), noting what was dropped. Diagnostics put the cause at the end.
pub fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    format!("[… {} earlier bytes omitted]
{}", start, &s[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_secret_shapes() {
        let t = "key sk-ant-api03-abcdefghijklmnopqrstuvwx and ghp_abcdefghijklmnopqrstuvwxyz0123 AKIAABCDEFGHIJKLMNOP";
        let r = redact_str(t);
        assert!(!r.contains("abcdefghijklmnop"), "{r}");
        assert_eq!(r.matches("[REDACTED]").count(), 3, "{r}");
    }

    #[test]
    fn redacts_an_unfinished_private_key_block() {
        let input = ["before\n-----BEGIN ", "PRIVATE KEY-----\nprivate-fixture-value"].concat();
        let output = redact_str(&input);
        assert!(output.contains("before") && output.contains("[REDACTED]"));
        assert!(!output.contains("private-fixture-value"));
    }

    #[test]
    fn redacts_assignments_but_keeps_key_name() {
        let r = redact_str(r#"API_KEY="hunter2hunter2" password: swordfish99"#);
        assert!(r.contains("API_KEY") && r.contains("password"), "{r}");
        assert!(!r.contains("hunter2") && !r.contains("swordfish99"), "{r}");
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        let t = "refactor the token bucket limiter in src/limit.rs";
        assert_eq!(redact_str(t), t);
    }

    #[test]
    fn walks_nested_json() {
        let mut v = serde_json::json!({"a":["Bearer abcdefghijklmnopqrstuvwxyz123"],"b":{"c":"ok"}});
        redact_value(&mut v);
        assert_eq!(v["a"][0], "[REDACTED]");
        assert_eq!(v["b"]["c"], "ok");
    }

    #[test]
    fn tail_keeps_the_end_where_the_cause_is() {
        let t = tail(&format!("{}ROOT CAUSE", "x".repeat(5000)), 100);
        assert!(t.ends_with("ROOT CAUSE") && t.contains("earlier bytes omitted"));
        assert_eq!(tail("short", 100), "short");
    }

    #[test]
    fn truncate_respects_utf8() {
        let (t, cut) = truncate("ééééé", 3);
        assert!(cut && t.starts_with('é'));
    }
}
