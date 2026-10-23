//! The filesystem boundary for the wiki and for a connected Obsidian vault.
//!
//! Rules (from the independent design review): every access goes through `safe_file`, which resolves the real path,
//! rejects reparse points / symlinks anywhere between the root and the file, rejects hardlinked files, and refuses
//! anything under `.obsidian/` or `.git/`. Writers take a per-root lock. This is a *check-then-use* boundary, not a
//! race-free one: a link swapped in between the check and the write cannot be ruled out without handle-based I/O, so
//! the managed tree should live somewhere only the user can write.

use crate::error::{ForgeError, Result};
use crate::paths;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Directories FORGE never reads from, writes to, or indexes inside a wiki root.
pub const FORBIDDEN_DIRS: &[&str] = &[".obsidian", ".git", ".trash", ".forge-quarantine", "node_modules"];

pub fn managed_root(home: &Path, project_id: &str) -> PathBuf {
    home.join("knowledge").join(project_id)
}

pub fn is_vault(path: &Path) -> bool {
    path.join(".obsidian").is_dir()
}

#[cfg(windows)]
fn is_reparse(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT: symlinks, junctions, mount points
}

#[cfg(not(windows))]
fn is_reparse(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

#[cfg(windows)]
pub(crate) fn link_count(path: &Path) -> std::io::Result<u32> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
    let f = File::open(path)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `f` is a valid open handle for the duration of the call and `info` is a properly sized out-parameter.
    let ok = unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut info) };
    if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(info.nNumberOfLinks) }
}

#[cfg(not(windows))]
pub(crate) fn link_count(path: &Path) -> std::io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::metadata(path)?.nlink() as u32)
}

/// Reject a path when anything between `root` and it (inclusive) is a link, or when a forbidden directory is crossed.
fn check_components(root: &Path, real: &Path) -> Result<()> {
    let rel = paths::relative_components(root, real).ok_or_else(|| ForgeError::PathRejected(format!("{} is outside the wiki root", real.display())))?;
    let mut cur = root.to_path_buf();
    for comp in &rel {
        if FORBIDDEN_DIRS.iter().any(|d| comp.eq_ignore_ascii_case(d)) {
            return Err(ForgeError::PathRejected(format!("`{comp}` is never touched by FORGE")));
        }
        cur.push(comp);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if is_reparse(&m) => return Err(ForgeError::PathRejected(format!("{} is a link/junction; managed knowledge trees must not contain links", cur.display()))),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break, // the tail does not exist yet
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Resolve `rel` under `root` for reading or writing a page file. Returns the real, link-free path.
pub fn safe_file(root: &Path, rel: &str) -> Result<PathBuf> {
    if rel.is_empty() || rel.starts_with('/') || rel.starts_with('\\') || rel.contains('\0') {
        return Err(ForgeError::PathRejected(format!("invalid page path \"{rel}\"")));
    }
    let canon_root = dunce::canonicalize(root).map_err(|e| ForgeError::PathRejected(format!("wiki root not accessible: {e}")))?;
    // Walk the path as SPELLED first: canonicalising resolves an internal junction/symlink away and would hide it.
    let spelled = paths::lexical_normalize(&canon_root.join(rel));
    check_components(&canon_root, &spelled)?;
    let real = paths::resolve_within(&canon_root, Path::new(rel))?;
    check_components(&canon_root, &real)?;
    if real.is_dir() {
        return Err(ForgeError::PathRejected(format!("{} is a directory", real.display())));
    }
    if real.exists() {
        let m = std::fs::symlink_metadata(&real)?;
        if !m.is_file() {
            return Err(ForgeError::PathRejected(format!("{} is not a regular file", real.display())));
        }
        if link_count(&real)? > 1 {
            return Err(ForgeError::PathRejected(format!("{} has multiple hard links; FORGE will not read or write it", real.display())));
        }
    }
    Ok(real)
}

/// Is `dir` (a directory inside the root) one FORGE may descend into when scanning?
pub fn scannable_dir(name: &str, meta: &std::fs::Metadata) -> bool {
    !name.starts_with('.') && !FORBIDDEN_DIRS.iter().any(|d| name.eq_ignore_ascii_case(d)) && !is_reparse(meta)
}

pub fn is_reparse_meta(meta: &std::fs::Metadata) -> bool {
    is_reparse(meta)
}

pub fn hardlinked(path: &Path) -> bool {
    link_count(path).map(|n| n > 1).unwrap_or(true)
}

/// Per-root writer lock (an OS file lock, so it also coordinates separate FORGE processes). Released on drop.
pub struct WriterLock {
    _file: File,
}

pub fn writer_lock(root: &Path) -> Result<WriterLock> {
    std::fs::create_dir_all(root)?;
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(root.join(".forge-lock"))?;
    let t0 = Instant::now();
    loop {
        match f.try_lock() {
            Ok(()) => return Ok(WriterLock { _file: f }),
            Err(std::fs::TryLockError::WouldBlock) if t0.elapsed() < Duration::from_secs(10) => std::thread::sleep(Duration::from_millis(25)),
            Err(std::fs::TryLockError::WouldBlock) => return Err(ForgeError::Other("another FORGE process is writing this knowledge folder; try again".into())),
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
    }
}

/// `obsidian://open?path=<absolute file path>` — works for any vault Obsidian already knows.
pub fn obsidian_uri(file: &Path) -> String {
    let p = file.to_string_lossy().replace('\\', "/");
    let enc: String = p
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    format!("obsidian://open?path={enc}")
}

/// Is a handler for the `obsidian://` scheme registered? (No assumptions that Obsidian is running or its CLI exists.)
pub fn obsidian_installed() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("reg")
            .args(["query", r"HKCU\Software\Classes\obsidian", "/ve"])
            .creation_flags(0x0800_0000)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
            || std::process::Command::new("reg")
                .args(["query", r"HKCR\obsidian", "/ve"])
                .creation_flags(0x0800_0000)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("pages")).unwrap();
        d
    }

    #[test]
    fn ordinary_paths_resolve_and_traversal_is_refused() {
        let r = root();
        assert!(safe_file(r.path(), "pages/a.md").is_ok());
        assert!(safe_file(r.path(), "pages/new/deep/b.md").is_ok());
        for bad in ["../x.md", "pages/../../x.md", "/etc/passwd", "\\\\server\\share\\x.md", "", "pages/a.md\0.png"] {
            assert!(safe_file(r.path(), bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn obsidian_and_git_directories_are_never_touched() {
        let r = root();
        for p in [".obsidian/plugins/evil/main.js", ".obsidian/app.json", ".git/config", ".OBSIDIAN/x.md", "pages/node_modules/x.md"] {
            assert!(safe_file(r.path(), p).is_err(), "{p}");
        }
    }

    #[test]
    fn a_directory_junction_inside_the_tree_is_refused() {
        let r = root();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.md"), "outside").unwrap();
        let link = r.path().join("pages").join("shortcut");
        #[cfg(windows)]
        {
            let out = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(outside.path()).output().unwrap();
            assert!(out.status.success(), "{out:?}");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let err = safe_file(r.path(), "pages/shortcut/secret.md").unwrap_err().to_string();
        assert!(err.contains("outside") || err.contains("link") || err.contains("resolves"), "{err}");
    }

    #[test]
    fn a_junction_that_points_elsewhere_inside_the_same_tree_is_still_refused() {
        // Canonicalising `pages/alias` yields `pages/real`, which looks innocent; only the spelled path reveals the link.
        let r = root();
        std::fs::create_dir_all(r.path().join("pages").join("real")).unwrap();
        std::fs::write(r.path().join("pages").join("real").join("x.md"), "x").unwrap();
        let alias = r.path().join("pages").join("alias");
        #[cfg(windows)]
        {
            let out = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&alias).arg(r.path().join("pages").join("real")).output().unwrap();
            assert!(out.status.success(), "{out:?}");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(r.path().join("pages").join("real"), &alias).unwrap();
        assert!(safe_file(r.path(), "pages/real/x.md").is_ok());
        let err = safe_file(r.path(), "pages/alias/x.md").unwrap_err().to_string();
        assert!(err.contains("link"), "{err}");
    }

    #[test]
    fn hardlinked_files_are_refused() {
        let r = root();
        let a = r.path().join("pages").join("a.md");
        std::fs::write(&a, "x").unwrap();
        assert!(safe_file(r.path(), "pages/a.md").is_ok());
        let b = r.path().join("pages").join("b.md");
        std::fs::hard_link(&a, &b).unwrap();
        let err = safe_file(r.path(), "pages/b.md").unwrap_err().to_string();
        assert!(err.contains("hard links"), "{err}");
        assert!(safe_file(r.path(), "pages/a.md").is_err(), "both names of a hardlinked file are refused");
    }

    #[test]
    fn writer_lock_excludes_a_second_writer_until_released() {
        let r = root();
        let first = writer_lock(r.path()).unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(r.path().join(".forge-lock")).unwrap();
        assert!(matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock)), "a second handle must not get the lock");
        drop(first);
        assert!(f.try_lock().is_ok());
    }

    #[test]
    fn vault_detection_and_obsidian_uri_encoding() {
        let r = root();
        assert!(!is_vault(r.path()));
        std::fs::create_dir(r.path().join(".obsidian")).unwrap();
        assert!(is_vault(r.path()));
        let u = obsidian_uri(Path::new(r"C:\Users\me\My Vault\Note #1.md"));
        assert_eq!(u, "obsidian://open?path=C%3A/Users/me/My%20Vault/Note%20%231.md");
    }

    #[test]
    fn scan_skips_hidden_forbidden_and_linked_directories() {
        let r = root();
        std::fs::create_dir(r.path().join(".obsidian")).unwrap();
        let m = std::fs::metadata(r.path().join(".obsidian")).unwrap();
        assert!(!scannable_dir(".obsidian", &m) && !scannable_dir(".hidden", &m) && !scannable_dir("node_modules", &m));
        assert!(scannable_dir("pages", &std::fs::metadata(r.path().join("pages")).unwrap()));
    }
}
