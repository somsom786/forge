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
    let canon_root = dunce::canonicalize(root)
        .map_err(|e| ForgeError::PathRejected(format!("root not accessible: {e}")))?;
    // The root itself may be spelled with an 8.3 alias or another case (a profile under `C:\Users\RUNNER~1` is common for long
    // user names), and an agent may echo that spelling back. Only the part *below* the root is the caller's choice, so only
    // that part has to pass the spelling rules; the root is compared by its real path.
    let below: PathBuf = if candidate.is_absolute() {
        let lex = lexical_normalize(candidate);
        match relative_components(root, &lex).or_else(|| relative_components(&canon_root, &lex)) {
            Some(parts) => parts.iter().collect(),
            None => {
                reject_suspicious(candidate)?;
                return Err(ForgeError::PathRejected(format!("{} escapes {}", lex.display(), canon_root.display())));
            }
        }
    } else {
        candidate.to_path_buf()
    };
    reject_suspicious(&below)?;
    let joined = canon_root.join(&below);
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
        // `exists()` follows links, so a *broken* link looks absent. It is not: writing through it creates its target, which
        // can be anywhere (outside the worktree, or a protected file inside it). Refuse any link on the not-yet-existing part.
        if std::fs::symlink_metadata(&probe).is_ok() {
            return Err(ForgeError::PathRejected(format!("{} is a link whose target does not exist; FORGE does not write through it", probe.display())));
        }
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

/// Test helper: a directory link at `link` pointing to `target` — a junction on Windows (no privilege needed), a symlink on
/// Unix. Both are what FORGE must refuse to follow.
#[cfg(test)]
pub(crate) fn test_dir_link(link: &Path, target: &Path) {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link).arg(target).output().unwrap();
        assert!(out.status.success(), "junction creation failed: {out:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
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

    /// Found by CI on a GitHub Windows runner, whose temp folder is `C:\Users\RUNNER~1\...`: a worktree whose *own* path is
    /// spelled with an 8.3 alias denied every write inside it. Spelling rules apply below the root only.
    #[cfg(windows)]
    #[test]
    fn a_root_spelled_with_a_short_name_still_contains_its_files_but_short_names_below_it_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let long = d.path().join("averyveryverylongname");
        std::fs::create_dir_all(long.join("src")).unwrap();
        std::fs::create_dir_all(long.join("subdirectory_long")).unwrap();
        // The short form of `long`, if the volume has 8.3 names enabled (asked from Windows itself, no shell involved).
        let short = {
            use std::os::windows::ffi::{OsStrExt, OsStringExt};
            let wide: Vec<u16> = long.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut buf = vec![0u16; 1024];
            // SAFETY: `wide` is NUL-terminated and `buf` is a writable buffer of the length passed.
            let n = unsafe { windows_sys::Win32::Storage::FileSystem::GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
            assert!(n > 0 && n < buf.len(), "GetShortPathNameW failed for {}", long.display());
            PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]))
        };
        if !short.to_string_lossy().contains('~') {
            return; // 8.3 names are disabled on this volume; nothing to test
        }
        let abs = resolve_within(&short, &short.join("src").join("a.rs"));
        assert!(abs.is_ok(), "an absolute path under the short-spelled root {}: {abs:?}", short.display());
        let rel = resolve_within(&short, Path::new("src/a.rs"));
        assert!(rel.is_ok(), "a relative path under the short-spelled root {}: {rel:?}", short.display());
        // A short spelling of a root the caller named in full is not guessed at: it is refused (fail closed).
        assert!(resolve_within(&long, &short.join("src").join("a.rs")).is_err());
        assert!(resolve_within(&short, Path::new("SUBDIR~1/x")).is_err(), "a short name below the root is still refused");
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

    /// Security review F1: a link to a path that does not exist yet used to resolve to the link's own (inside) path.
    #[test]
    fn a_broken_link_is_refused_wherever_it_points() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let gone = outside.path().join("not-yet").join("autostart");
        // A dangling directory link (a junction on Windows: create it to a real directory, then remove the target).
        std::fs::create_dir_all(&gone).unwrap();
        test_dir_link(&d.path().join("notes"), &gone);
        std::fs::remove_dir_all(outside.path().join("not-yet")).unwrap();
        assert!(resolve_within(d.path(), Path::new("notes")).is_err(), "the dangling link itself");
        assert!(resolve_within(d.path(), Path::new("notes/x.desktop")).is_err(), "a path through it");
        #[cfg(unix)]
        {
            // A dangling *file* link, to outside and to a protected file inside the worktree.
            std::os::unix::fs::symlink(outside.path().join("victim.txt"), d.path().join("a.md")).unwrap();
            assert!(resolve_within(d.path(), Path::new("a.md")).is_err());
            std::fs::create_dir_all(d.path().join(".claude")).unwrap();
            std::os::unix::fs::symlink(d.path().join(".claude/settings.json"), d.path().join("b.md")).unwrap();
            assert!(resolve_within(d.path(), Path::new("b.md")).is_err());
        }
        assert!(resolve_within(d.path(), Path::new("fresh/new.txt")).is_ok(), "plain new paths still resolve");
    }
}
