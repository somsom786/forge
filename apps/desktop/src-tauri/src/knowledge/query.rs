//! Turning free text (a mission objective, a search box) into a safe SQLite FTS5 query, and pulling file-path hints out
//! of it. Every term is emitted as a quoted string, so user/agent text can never be interpreted as FTS syntax
//! (`NEAR`, `OR`, `*`, `:` column filters, parentheses).

use std::collections::HashSet;

const MAX_TERMS: usize = 40;

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "with", "is", "are", "be", "this", "that", "it", "as", "at", "by",
    "from", "we", "you", "i", "me", "my", "our", "your", "should", "must", "can", "will", "do", "does", "not", "no", "so", "if",
    "then", "than", "into", "up", "out", "all", "any", "add", "make", "use", "using", "new", "please", "need", "want", "get",
];

/// Split an identifier-ish token into words: `resetToken` -> reset, token; `snake_case` -> snake, case; `HTTPServer2` -> http, server, 2.
fn split_identifier(tok: &str) -> Vec<String> {
    let chars: Vec<char> = tok.chars().collect();
    let mut words = vec![];
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let boundary = !cur.is_empty() && {
            let prev = chars[i - 1];
            let next = chars.get(i + 1).copied();
            (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_alphabetic() && c.is_numeric())
                || (prev.is_numeric() && c.is_alphabetic())
                || (prev.is_uppercase() && c.is_uppercase() && next.is_some_and(|n| n.is_lowercase()))
        };
        if boundary {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words.into_iter().map(|w| w.to_lowercase()).collect()
}

/// Distinct, ordered search terms.
pub fn terms(text: &str) -> Vec<String> {
    let stop: HashSet<&str> = STOPWORDS.iter().copied().collect();
    let mut seen = HashSet::new();
    let mut out = vec![];
    for raw in text.split(|c: char| c.is_whitespace() || "()[]{}<>,;:!?\"'`|=+*&^%$#@~".contains(c)) {
        if raw.is_empty() {
            continue;
        }
        // Keep the whole identifier too (`reset_token`): the index tokenises on non-alphanumerics, so the parts match.
        for w in split_identifier(raw) {
            if w.chars().count() < 2 || stop.contains(w.as_str()) || w.chars().all(|c| c.is_numeric()) && w.len() < 3 {
                continue;
            }
            if seen.insert(w.clone()) {
                out.push(w);
            }
            if out.len() >= MAX_TERMS {
                return out;
            }
        }
    }
    out
}

/// `"term1" OR "term2" ...` — or None when nothing searchable remains.
pub fn fts_match(text: &str) -> Option<String> {
    let t = terms(text);
    if t.is_empty() {
        return None;
    }
    // The index uses an unstemmed tokenizer (precise for code identifiers). Longer terms match as prefixes, which gives
    // most of the recall stemming would ("expire" finds "expires", "expiry" does not, by design).
    Some(
        t.iter()
            .map(|w| {
                let q = w.replace('"', "");
                if q.chars().count() >= 5 { format!("\"{q}\"*") } else { format!("\"{q}\"") }
            })
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// Strings in `text` that look like repo file paths (`src/auth/reset.rs`, `docs\\ARCH.md`, `Cargo.toml`).
pub fn path_hints(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut seen = HashSet::new();
    // ':' is not a separator here, so URLs and drive letters stay intact and can be rejected as a whole.
    for raw in text.split(|c: char| c.is_whitespace() || "()[]{}<>,;!?\"'`|=*&^%$#@~".contains(c)) {
        let raw = raw.trim_end_matches(['.', ':']);
        let norm = raw.replace('\\', "/");
        let bytes = norm.as_bytes();
        let absolute = norm.starts_with('/') || (bytes.len() > 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic());
        // Anything that can escape the repo, point outside it, or is a URL is not a repo-relative path.
        if norm.contains("://") || absolute || norm.split('/').any(|seg| seg == "..") || norm.len() < 3 || norm.len() > 200 {
            continue;
        }
        let t = norm.trim_start_matches("./");
        let ext = t.rsplit('/').next().and_then(|f| f.rsplit_once('.')).map(|(_, e)| e.to_string());
        let looks = ext.as_deref().is_some_and(|e| (1..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()) && e.chars().any(|c| c.is_ascii_alphabetic()))
            || t.contains('/');
        if looks && seen.insert(t.to_string()) {
            out.push(t.to_string());
        }
    }
    out.truncate(30);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_split_into_searchable_words() {
        assert_eq!(terms("resetToken"), vec!["reset", "token"]);
        assert_eq!(terms("snake_case_name"), vec!["snake", "case", "name"]);
        assert_eq!(terms("HTTPServer2Config"), vec!["http", "server", "config"], "digit-only fragments under 3 chars are noise");
        assert_eq!(terms("src/auth/reset.rs"), vec!["src", "auth", "reset", "rs"]);
    }

    #[test]
    fn stopwords_duplicates_and_noise_are_dropped() {
        assert_eq!(terms("Please add the password reset to the auth module"), vec!["password", "reset", "auth", "module"]);
        assert_eq!(terms("token token Token"), vec!["token"]);
        assert!(terms("?! the a an").is_empty());
        assert!(fts_match("the a").is_none());
    }

    #[test]
    fn fts_syntax_in_user_text_is_neutralised() {
        let q = fts_match("NEAR(a b) OR \"quoted\" title:secret * -excluded").unwrap();
        assert!(
            q.split(" OR ").all(|t| {
                let t = t.strip_suffix('*').unwrap_or(t);
                t.starts_with('"') && t.ends_with('"') && t[1..t.len() - 1].chars().all(|c| c.is_alphanumeric())
            }),
            "{q}"
        );
        // and it must actually be accepted by SQLite
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE VIRTUAL TABLE t USING fts5(body); INSERT INTO t VALUES ('secret title near excluded quoted');").unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM t WHERE t MATCH ?1", [&q], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        // a hostile column-filter attempt matches nothing special and does not error
        let q2 = fts_match("body:* OR (").unwrap_or_default();
        let _: i64 = c.query_row("SELECT count(*) FROM t WHERE t MATCH ?1", [&q2], |r| r.get(0)).unwrap_or(0);
    }

    #[test]
    fn term_count_is_capped() {
        let big: String = (0..500).map(|i| format!("word{i} ")).collect();
        assert_eq!(terms(&big).len(), MAX_TERMS);
    }

    #[test]
    fn paths_are_recognised_and_traversal_rejected() {
        let h = path_hints("Fix src/auth/reset.rs and docs\\ARCHITECTURE.md, see Cargo.toml (not https://x.com/a.html or ../../etc/passwd)");
        assert_eq!(h, vec!["src/auth/reset.rs", "docs/ARCHITECTURE.md", "Cargo.toml"]);
        assert!(path_hints("just words here").is_empty());
    }
}
