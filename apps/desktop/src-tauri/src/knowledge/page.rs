//! Knowledge page files: strict frontmatter subset, slugs, wikilinks, content normalisation, atomic writes.
//!
//! Frontmatter is deliberately NOT full YAML. Obsidian "properties" are YAML, but a general YAML parser brings anchors,
//! aliases, custom tags and unbounded expansion that have no business in a file an agent will read. We accept a small
//! subset (scalar, inline list, block list), ignore everything else, and report what we ignored.

use crate::error::{ForgeError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

pub const MAX_PAGE_BYTES: usize = 256 * 1024;
const MAX_FRONTMATTER_BYTES: usize = 16 * 1024;
const MAX_LIST_ITEMS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PageState {
    Proposed,
    Verified,
    Disputed,
    Superseded,
    Archived,
}

impl PageState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Verified => "verified",
            Self::Disputed => "disputed",
            Self::Superseded => "superseded",
            Self::Archived => "archived",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "proposed" => Self::Proposed,
            "verified" => Self::Verified,
            "disputed" => Self::Disputed,
            "superseded" => Self::Superseded,
            "archived" => Self::Archived,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    User,
    Agent,
    System,
}

impl Author {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::System => "system",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "user" => Self::User,
            "agent" => Self::Agent,
            "system" => Self::System,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub id: Option<String>,
    pub title: Option<String>,
    pub kind: Option<String>,
    pub state: Option<String>,
    pub scope: Option<String>,
    pub author: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub topic: Option<String>,
    pub supersedes: Option<String>,
    pub sources: Vec<String>,
    pub files: Vec<String>,
    pub tags: Vec<String>,
    /// Unknown keys are preserved verbatim so a user's own properties survive a rewrite.
    pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub meta: Meta,
    pub body: String,
    pub had_frontmatter: bool,
    /// Constructs that were ignored (anchors, tags, nested maps...).
    pub warnings: Vec<String>,
}

/// Normalise for hashing/comparison: strip BOM, CRLF/CR -> LF, trailing whitespace at the end.
pub fn normalize(text: &str) -> String {
    let t = text.strip_prefix('\u{feff}').unwrap_or(text);
    let t = t.replace("\r\n", "\n").replace('\r', "\n");
    format!("{}\n", t.trim_end())
}

pub fn content_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(normalize(text).as_bytes()))
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))) {
        let inner = &v[1..v.len() - 1];
        if v.starts_with('"') {
            return inner.replace("\\\"", "\"").replace("\\\\", "\\");
        }
        return inner.replace("''", "'");
    }
    v.to_string()
}

fn parse_inline_list(v: &str) -> Vec<String> {
    let inner = v.trim().trim_start_matches('[').trim_end_matches(']');
    let mut out = vec![];
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in inner.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"') | (None, '\'') => quote = Some(ch),
            (None, ',') => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out.truncate(MAX_LIST_ITEMS);
    out
}

/// Parse a page. Never fails: malformed or hostile frontmatter degrades to "no frontmatter" plus warnings.
pub fn parse(text: &str) -> Parsed {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let norm = text.replace("\r\n", "\n");
    let mut warnings = vec![];
    let mut meta = Meta::default();
    let Some(rest) = norm.strip_prefix("---\n") else {
        return Parsed { meta, body: norm, had_frontmatter: false, warnings };
    };
    let Some(end) = rest.find("\n---\n").or_else(|| rest.strip_suffix("\n---").map(|r| r.len())) else {
        warnings.push("frontmatter is not closed; treated as body text".into());
        return Parsed { meta, body: norm, had_frontmatter: false, warnings };
    };
    let fm = &rest[..end];
    if fm.len() > MAX_FRONTMATTER_BYTES {
        warnings.push("frontmatter too large; ignored".into());
        let body_start = (end + 5).min(rest.len());
        return Parsed { meta, body: rest[body_start..].to_string(), had_frontmatter: true, warnings };
    }
    let body = rest.get(end + 5..).unwrap_or("").to_string();

    let lines: Vec<&str> = fm.lines().collect();
    let mut seen_keys = std::collections::HashSet::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') || line.starts_with("- ") {
            warnings.push(format!("unexpected indentation ignored: {}", line.trim()));
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            warnings.push(format!("line without a key ignored: {}", line.trim()));
            continue;
        };
        let key = k.trim().to_string();
        if !seen_keys.insert(key.clone()) {
            warnings.push(format!("duplicate key `{key}` ignored (the first one wins)"));
            // Skip a block list that belongs to the duplicate, too.
            while i < lines.len() && (lines[i].starts_with(' ') || lines[i].starts_with("- ")) {
                i += 1;
            }
            continue;
        }
        let mut val = v.trim().to_string();
        if val.contains('&') && val.starts_with('&') || val.starts_with('*') || val.starts_with('!') || val.starts_with("<<") {
            warnings.push(format!("YAML anchor/alias/tag in `{key}` ignored"));
            continue;
        }
        let mut list: Option<Vec<String>> = None;
        if val.is_empty() {
            // block list
            let mut items = vec![];
            while i < lines.len() && (lines[i].trim_start().starts_with("- ") || lines[i].trim() == "-") && (lines[i].starts_with(' ') || lines[i].starts_with('-')) {
                items.push(unquote(lines[i].trim_start().trim_start_matches('-').trim()));
                i += 1;
                if items.len() >= MAX_LIST_ITEMS {
                    break;
                }
            }
            if !items.is_empty() {
                list = Some(items);
            }
        } else if val.starts_with('[') && val.ends_with(']') {
            list = Some(parse_inline_list(&val));
        } else if val.starts_with('{') || val.starts_with('|') || val.starts_with('>') {
            warnings.push(format!("nested/multiline value for `{key}` ignored"));
            continue;
        } else {
            val = unquote(&val);
        }
        let scalar = |v: &str| if v.is_empty() { None } else { Some(v.to_string()) };
        match (key.as_str(), list) {
            ("id", None) => meta.id = scalar(&val),
            ("title", None) => meta.title = scalar(&val),
            ("kind", None) => meta.kind = scalar(&val),
            ("state", None) => meta.state = scalar(&val),
            ("scope", None) => meta.scope = scalar(&val),
            ("author", None) => meta.author = scalar(&val),
            ("created", None) => meta.created = scalar(&val),
            ("updated", None) => meta.updated = scalar(&val),
            ("topic", None) => meta.topic = scalar(&val),
            ("supersedes", None) => meta.supersedes = scalar(&val),
            ("sources", Some(l)) => meta.sources = l,
            ("files", Some(l)) => meta.files = l,
            ("tags", Some(l)) => meta.tags = l,
            ("sources" | "files" | "tags", None) => {
                let single = scalar(&val).into_iter().collect::<Vec<_>>();
                match key.as_str() {
                    "sources" => meta.sources = single,
                    "files" => meta.files = single,
                    _ => meta.tags = single,
                }
            }
            (_, Some(l)) => {
                meta.extra.insert(key, format!("[{}]", l.join(", ")));
            }
            (_, None) => {
                meta.extra.insert(key, val);
            }
        }
    }
    Parsed { meta, body, had_frontmatter: true, warnings }
}

fn q(s: &str) -> String {
    let needs = s.is_empty() || s.contains([':', '#', '"', '\'', '[', ']', '{', '}', ',', '&', '*', '!', '|', '>', '%', '@', '`', '\n']) || s.starts_with(['-', '?', ' ']) || s.ends_with(' ');
    if needs { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ")) } else { s.to_string() }
}

fn list(key: &str, items: &[String], out: &mut String) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("{key}:\n"));
    for it in items {
        out.push_str(&format!("  - {}\n", q(it)));
    }
}

/// Write frontmatter + body. Output is stable (fixed key order) so unchanged pages re-serialise identically.
pub fn serialize(meta: &Meta, body: &str) -> String {
    let mut o = String::from("---\n");
    for (k, v) in [
        ("id", &meta.id), ("title", &meta.title), ("kind", &meta.kind), ("state", &meta.state), ("scope", &meta.scope),
        ("author", &meta.author), ("created", &meta.created), ("updated", &meta.updated), ("topic", &meta.topic), ("supersedes", &meta.supersedes),
    ] {
        if let Some(v) = v {
            o.push_str(&format!("{k}: {}\n", q(v)));
        }
    }
    list("sources", &meta.sources, &mut o);
    list("files", &meta.files, &mut o);
    list("tags", &meta.tags, &mut o);
    for (k, v) in &meta.extra {
        o.push_str(&format!("{k}: {v}\n"));
    }
    o.push_str("---\n");
    o.push_str(body.trim_start_matches('\n'));
    if !o.ends_with('\n') {
        o.push('\n');
    }
    o
}

/// File-name-safe, stable, ASCII slug.
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut dash = true;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
        if out.len() >= 60 {
            break;
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() { "page".into() } else { s }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    pub target: String,
    pub alias: Option<String>,
    pub heading: Option<String>,
}

/// `[[Target]]`, `[[Target|alias]]`, `[[Target#Heading]]`. Links inside fenced code are ignored.
pub fn wikilinks(body: &str) -> Vec<WikiLink> {
    let mut out = vec![];
    let mut in_fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut rest = line;
        while let Some(s) = rest.find("[[") {
            let after = &rest[s + 2..];
            let Some(e) = after.find("]]") else { break };
            let inner = &after[..e];
            rest = &after[e + 2..];
            if inner.is_empty() || inner.len() > 200 || inner.contains(['[', ']', '\n']) {
                continue;
            }
            let (left, alias) = match inner.split_once('|') {
                Some((l, a)) => (l, Some(a.trim().to_string())),
                None => (inner, None),
            };
            let (target, heading) = match left.split_once('#') {
                Some((t, h)) => (t.trim(), Some(h.trim().to_string())),
                None => (left.trim(), None),
            };
            if !target.is_empty() {
                out.push(WikiLink { target: target.to_string(), alias, heading });
            }
        }
    }
    out
}

/// Write `bytes` to `path` atomically: temp file in the same directory, flushed to disk, then renamed over the target.
/// A crash leaves either the old file or the new one, never a torn mix.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_PAGE_BYTES {
        return Err(ForgeError::Other(format!("page is too large ({} bytes, limit {MAX_PAGE_BYTES})", bytes.len())));
    }
    let dir = path.parent().ok_or_else(|| ForgeError::PathRejected("no parent directory".into()))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".forge-tmp-{}", uuid::Uuid::now_v7()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_meta_body_and_unknown_user_properties() {
        let meta = Meta {
            id: Some("01abc".into()),
            title: Some("Why SQLite: WAL, not Postgres".into()),
            kind: Some("decision".into()),
            state: Some("verified".into()),
            author: Some("user".into()),
            sources: vec!["commit:abc123".into(), "doc:docs/ARCHITECTURE.md".into()],
            files: vec!["src-tauri/src/db.rs".into()],
            tags: vec!["database".into()],
            extra: [("cssclass".to_string(), "wide".to_string())].into(),
            ..Default::default()
        };
        let text = serialize(&meta, "# Heading\n\nBody with [[Other Page]].\n");
        let p = parse(&text);
        assert!(p.had_frontmatter && p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(p.meta, meta);
        assert_eq!(p.body, "# Heading\n\nBody with [[Other Page]].\n");
        assert_eq!(serialize(&p.meta, &p.body), text, "stable re-serialisation");
    }

    #[test]
    fn pages_without_frontmatter_are_plain_bodies() {
        let p = parse("# Just notes\n\nNo properties here.\n");
        assert!(!p.had_frontmatter);
        assert_eq!(p.meta, Meta::default());
        assert!(p.body.starts_with("# Just notes"));
    }

    #[test]
    fn crlf_bom_and_unclosed_frontmatter_are_handled() {
        let p = parse("\u{feff}---\r\ntitle: Win\r\nstate: proposed\r\n---\r\nbody\r\n");
        assert_eq!(p.meta.title.as_deref(), Some("Win"));
        assert_eq!(p.meta.state.as_deref(), Some("proposed"));
        let u = parse("---\ntitle: never closed\nstill going\n");
        assert!(!u.had_frontmatter && !u.warnings.is_empty());
        assert!(u.body.contains("never closed"), "an unclosed block is just text");
    }

    #[test]
    fn hostile_yaml_constructs_are_ignored_and_reported_not_executed() {
        let text = "---\ntitle: &a anchored\nkind: *a\nstate: !!python/object:os.system x\nscope: {a: b}\ntopic: |\n  multi\nauthor: user\n---\nbody";
        let p = parse(text);
        assert_eq!(p.meta.author.as_deref(), Some("user"));
        assert_eq!(p.meta.title, None);
        assert_eq!(p.meta.kind, None);
        assert_eq!(p.meta.state, None, "a tag-bearing value must not become a state");
        assert!(p.warnings.len() >= 4, "{:?}", p.warnings);
    }

    #[test]
    fn duplicate_keys_cannot_shadow_the_first_value() {
        let p = parse("---\nauthor: user\nauthor: system\nfiles:\n  - a.rs\nfiles:\n  - evil.rs\n---\nb");
        assert_eq!(p.meta.author.as_deref(), Some("user"));
        assert_eq!(p.meta.files, vec!["a.rs"]);
        assert_eq!(p.warnings.iter().filter(|w| w.contains("duplicate key")).count(), 2);
    }

    #[test]
    fn oversized_frontmatter_and_lists_are_bounded() {
        let big = format!("---\ntitle: x\nnotes: {}\n---\nbody", "a".repeat(MAX_FRONTMATTER_BYTES + 10));
        let p = parse(&big);
        assert!(p.meta.title.is_none() && p.warnings.iter().any(|w| w.contains("too large")));
        let items: Vec<String> = (0..1000).map(|i| format!("  - f{i}")).collect();
        let l = parse(&format!("---\nfiles:\n{}\n---\nb", items.join("\n")));
        assert!(l.meta.files.len() <= MAX_LIST_ITEMS);
    }

    #[test]
    fn inline_and_block_lists_and_quoted_values() {
        let p = parse("---\ntags: [a, \"b, c\", 'd']\nfiles:\n  - src/a.rs\n  - \"weird: name.rs\"\ntitle: \"Quote \\\" inside\"\n---\n");
        assert_eq!(p.meta.tags, vec!["a", "b, c", "d"]);
        assert_eq!(p.meta.files, vec!["src/a.rs", "weird: name.rs"]);
        assert_eq!(p.meta.title.as_deref(), Some("Quote \" inside"));
    }

    #[test]
    fn serializer_quotes_values_that_would_otherwise_inject_keys() {
        let meta = Meta { title: Some("evil\nstate: verified".into()), kind: Some("note: with colon".into()), ..Default::default() };
        let p = parse(&serialize(&meta, "x"));
        assert_eq!(p.meta.state, None, "a title cannot smuggle in a state line");
        assert_eq!(p.meta.title.as_deref(), Some("evil state: verified"));
        assert_eq!(p.meta.kind.as_deref(), Some("note: with colon"));
    }

    #[test]
    fn hash_ignores_line_endings_bom_and_trailing_whitespace_only() {
        assert_eq!(content_hash("a\nb\n"), content_hash("\u{feff}a\r\nb\r\n\r\n"));
        assert_ne!(content_hash("a\nb"), content_hash("a\nc"));
        assert_ne!(content_hash("a b"), content_hash("a  b"), "interior whitespace is meaningful");
    }

    #[test]
    fn slugs_are_safe_and_stable() {
        assert_eq!(slugify("Why we chose SQLite (WAL)!"), "why-we-chose-sqlite-wal");
        assert_eq!(slugify("../../etc/passwd"), "etc-passwd");
        assert_eq!(slugify("日本語"), "page");
        assert!(slugify(&"x".repeat(500)).len() <= 60);
    }

    #[test]
    fn wikilinks_support_alias_heading_and_skip_code_fences() {
        let l = wikilinks("See [[Auth Design]] and [[Db#Indexes|the indexes]].\n```\n[[not a link]]\n```\n[[ ]] [[a]]b]] [[Last]]");
        let targets: Vec<_> = l.iter().map(|w| w.target.as_str()).collect();
        assert_eq!(targets, vec!["Auth Design", "Db", "a", "Last"]);
        assert_eq!(l[1].alias.as_deref(), Some("the indexes"));
        assert_eq!(l[1].heading.as_deref(), Some("Indexes"));
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("pages").join("a.md");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
        let names: Vec<_> = std::fs::read_dir(p.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["a.md"]);
        assert!(atomic_write(&p, &vec![b'x'; MAX_PAGE_BYTES + 1]).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two", "a refused write leaves the old content");
    }
}
