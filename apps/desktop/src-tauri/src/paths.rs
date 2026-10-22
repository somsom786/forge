//! Path security. Every filesystem decision FORGE makes about "is this inside that"
//! goes through here. Windows semantics: case-insensitive, `\\?\` prefixes stripped,
//! alternate data streams and device names rejected, symlink/junction escapes caught
//! by canonicalising the deepest existing ancestor.

use crate::error::{ForgeError, Result};
use std::path::{Component, Path, PathBuf};

/// Resolve `.` and `..` lexically without touching the filesystem.
pub fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn comp_key(c: Component) -> String {
    let s = c.as_os_str().to_string_lossy().into_owned();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

/// Component-wise containment check; `root` itself counts as inside.
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    let r: Vec<String> = lexical_normalize(root).components().map(comp_key).collect();
    let c: Vec<String> = lexical_normalize(candidate).components().map(comp_key).collect();
    c.len() >= r.len() && c[..r.len()] == r[..]
}

fn short_name_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)^[^.~]{1,8}~[0-9]+(\.[^.]{0,3})?$").unwrap())
}

const DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn reject_suspicious(candidate: &Path) -> Result<()> {
    let s = candidate.to_string_lossy();
    if s.contains('\0') {
        return Err(ForgeError::PathRejected("NUL byte in path".into()));
    }
    if cfg!(windows) {
        // Past the drive prefix, ':' means an alternate data stream or a malformed path.
        let bytes = s.as_bytes();
        let skip = if s.starts_with(r"\\?\") { 4 } else { 0 };
        for (i, b) in bytes.iter().enumerate().skip(skip) {
            if *b == b':' && !(i == skip + 1 && bytes[skip].is_ascii_alphabetic()) {
                return Err(ForgeError::PathRejected(format!(
                    "alternate data stream or malformed path: {s}"
                )));
            }
        }
        for comp in candidate.components() {
            if let Component::Normal(n) = comp {
                let name = n.to_string_lossy();
                let lower = name.to_lowercase();
                let stem = lower.split('.').next().unwrap_or("");
                if DEVICE_NAMES.contains(&stem) {
                    return Err(ForgeError::PathRejected(format!("reserved device name: {stem}")));
                }
                // Windows silently strips trailing dots/spaces, so `.claude.` names `.claude`.
                if name.ends_with('.') || name.ends_with(' ') {
                    return Err(ForgeError::PathRejected(format!("ambiguous path component \"{name}\" (trailing dot or space)")));
                }
                if short_name_re().is_match(&name) {
                    return Err(ForgeError::PathRejected(format!("8.3 short-name alias \"{name}\" is not accepted; use the full name")));
                }
            }
        }
    }
    Ok(())
}

/// Resolve `candidate` (absolute, or relative to `root`) and prove it stays inside `root`,
/// following symlinks/junctions on the deepest existing ancestor.
pub fn resolve_within(root: &Path, candidate: &Path) -> Result<PathBuf> {
    reject_suspicious(candidate)?;
    let canon_root = dunce::canonicalize(root)
        .map_err(|e| ForgeError::PathRejected(format!("root not accessible: {e}")))?;
    let joined = if candidate.is_absolute() { candidate.to_path_buf() } else { canon_root.join(candidate) };
    let lex = lexical_normalize(&joined);
    if !is_within(&canon_root, &lex) {
        return Err(ForgeError::PathRejected(format!(
            "{} escapes {}",
            lex.display(),
            canon_root.display()
        )));
    }
    // Canonicalise the deepest existing ancestor (resolving junctions, symlinks and short names) and
    // re-attach the not-yet-existing tail. Callers must reason about THIS path, not the spelled one.
    let mut probe = lex.clone();
    let mut tail: Vec<std::ffi::OsString> = vec![];
    while !probe.exists() {
        match probe.file_name() {
            Some(n) => tail.push(n.to_os_string()),
            None => break,
        }
        if !probe.pop() {
            break;
        }
    }
    let mut real = if probe.exists() { dunce::canonicalize(&probe)? } else { probe };
    if !is_within(&canon_root, &real) {
        return Err(ForgeError::PathRejected(format!(
            "{} resolves outside {}",
            real.display(),
            canon_root.display()
        )));
    }
    for n in tail.iter().rev() {
        real.push(n);
    }
    Ok(real)
}

/// Components of `path` below `root` (original case), or None if it is not inside. Case-insensitive on Windows.
pub fn relative_components(root: &Path, path: &Path) -> Option<Vec<String>> {
    if !is_within(root, path) {
        return None;
    }
    let skip = lexical_normalize(root).components().count();
    Some(lexical_normalize(path).components().skip(skip).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect())
}

/// Normalise a user-supplied directory to a canonical absolute form without `\\?\`.
pub fn canonical_dir(p: &Path) -> Result<PathBuf> {
    let c = dunce::canonicalize(p)
        .map_err(|e| ForgeError::PathRejected(format!("{}: {e}", p.display())))?;
    if !c.is_dir() {
        return Err(ForgeError::PathRejected(format!("{} is not a directory", c.display())));
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> &'static Path {
        Path::new(if cfg!(windows) { r"C:\w\repo" } else { "/w/repo" })
    }

    #[test]
    fn lexical_dotdot_cannot_escape() {
        let evil = root().join("src").join("..").join("..").join("secrets");
        assert!(!is_within(root(), &evil));
        assert!(is_within(root(), &root().join("src").join("..").join("lib.rs")));
        assert!(is_within(root(), root()));
    }

    #[test]
    fn sibling_prefix_is_not_inside() {
        let sib = Path::new(if cfg!(windows) { r"C:\w\repo-evil\x" } else { "/w/repo-evil/x" });
        assert!(!is_within(root(), sib), "string-prefix match must not count as containment");
    }

    #[cfg(windows)]
    #[test]
    fn windows_case_insensitive_and_rejects_streams_and_devices() {
        assert!(is_within(Path::new(r"C:\Work\Repo"), Path::new(r"c:\work\repo\SRC\a.rs")));
        let d = tempfile::tempdir().unwrap();
        assert!(resolve_within(d.path(), Path::new("a.txt:hidden")).is_err());
        assert!(resolve_within(d.path(), Path::new("NUL")).is_err());
        assert!(resolve_within(d.path(), Path::new("sub/com1.txt")).is_err());
    }

    #[test]
    fn resolve_within_accepts_new_files_and_rejects_escape() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("src")).unwrap();
        assert!(resolve_within(d.path(), Path::new("src/new/file.rs")).is_ok());
        assert!(resolve_within(d.path(), Path::new("../outside.txt")).is_err());
        let outside = tempfile::tempdir().unwrap();
        assert!(resolve_within(d.path(), &outside.path().join("x")).is_err());
    }

    #[test]
    fn resolve_within_catches_directory_link_escape() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = d.path().join("link");
        #[cfg(windows)]
        {
            // Junctions need no privilege on Windows.
            let out = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(outside.path())
                .output()
                .unwrap();
            assert!(out.status.success(), "junction creation failed: {out:?}");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        assert!(resolve_within(d.path(), Path::new("link/file.txt")).is_err());
    }
}
