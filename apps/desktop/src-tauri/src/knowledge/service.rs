//! Knowledge operations over files + the index: scan, create, propose/apply revisions, sources, generated catalogue.
//!
//! Safety rules enforced here (each has a test):
//! * every file access goes through `vault::safe_file`;
//! * a page file is replaced only under the root's writer lock, only after re-reading it and matching the base hash,
//!   and with a journal entry so a crash can be recovered by inspecting hashes;
//! * edits to existing pages are *revisions* applied on request; FORGE never silently overwrites a page;
//! * `index.md` / `log.md` are generated from the database, never appended to;
//! * nothing is ingested from sensitive-looking files, and everything agent-supplied is redacted first.

use super::page::{self, Author, Meta};
use super::store::{IndexInput, KPage, KRevision, KSource};
use super::vault::{self, safe_file, writer_lock};
use crate::error::{ForgeError, Result};
use crate::git;
use crate::redact;
use crate::store::{Store, new_id, now};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const MAX_SCAN_FILES: usize = 5000;
const MAX_DEPTH: usize = 8;
const SNAPSHOT_BYTES: usize = 64 * 1024;
pub const KINDS: &[&str] = &["architecture", "component", "decision", "convention", "incident", "failure_mode", "solution", "debt", "experiment", "mission_record", "note"];

#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub store: &'a Store,
    pub home: &'a Path,
}

#[derive(Debug, Clone)]
pub struct RootInfo {
    pub kind: &'static str,
    pub path: PathBuf,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub indexed: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub changed_pages: Vec<String>,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NewPage {
    pub title: String,
    pub kind: String,
    pub body: String,
    pub files: Vec<String>,
    pub tags: Vec<String>,
    pub topic: Option<String>,
    pub author: Author,
    pub sources: Vec<SourceSpec>,
}

#[derive(Debug, Clone)]
pub enum SourceSpec {
    RepoFile { rel: String },
    Commit { sha: String },
    Mission { id: String, event_seq: Option<i64> },
    CheckRun { id: String },
    UserNote { text: String },
}

/// Names that should never be ingested into memory (or sent to a provider later).
pub fn sensitive_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n == ".env"
        || n.starts_with(".env.")
        || n.ends_with(".pem")
        || n.ends_with(".key")
        || n.ends_with(".pfx")
        || n.ends_with(".p12")
        || n.ends_with(".kdbx")
        || n.starts_with("id_rsa")
        || n.starts_with("id_ed25519")
        || n.contains("credential")
        || n.contains("secret")
        || n.contains("password")
        || n == ".npmrc"
        || n == ".pypirc"
        || n == ".netrc"
}

pub fn sensitive_path(rel: &str) -> bool {
    rel.split(['/', '\\']).any(sensitive_name)
}

// ---- roots ---------------------------------------------------------------------------------------

pub fn roots(ctx: Ctx, project_id: &str) -> Result<Vec<RootInfo>> {
    let managed = vault::managed_root(ctx.home, project_id);
    std::fs::create_dir_all(managed.join("pages"))?;
    let mut v = vec![RootInfo { kind: "managed", path: dunce::canonicalize(&managed)?, writable: true }];
    let (vp, write) = ctx.store.project_vault(project_id)?;
    if let Some(vp) = vp {
        let p = PathBuf::from(&vp);
        if p.is_dir() {
            v.push(RootInfo { kind: "vault", path: dunce::canonicalize(&p)?, writable: write });
        }
    }
    Ok(v)
}

pub fn root_for(ctx: Ctx, project_id: &str, kind: &str) -> Result<RootInfo> {
    roots(ctx, project_id)?.into_iter().find(|r| r.kind == kind).ok_or_else(|| ForgeError::NotFound(format!("the {kind} knowledge root is not available")))
}

/// Connect (or disconnect) an Obsidian vault folder. Existing vaults are read-only until write access is granted.
pub fn set_vault(ctx: Ctx, project_id: &str, path: Option<&Path>, write: bool) -> Result<()> {
    match path {
        None => ctx.store.set_project_vault(project_id, None, false),
        Some(p) => {
            let canon = crate::paths::canonical_dir(p)?;
            if crate::paths::is_within(ctx.home, &canon) || crate::paths::is_within(&canon, ctx.home) {
                return Err(ForgeError::PathRejected("the vault may not contain or live inside FORGE's own data directory".into()));
            }
            if canon.parent().is_none() {
                return Err(ForgeError::PathRejected("refusing to use a drive root as a vault".into()));
            }
            ctx.store.set_project_vault(project_id, Some(&canon.to_string_lossy()), write)?;
            ctx.store.audit("user", if write { "knowledge.vault.write_granted" } else { "knowledge.vault.connected" }, Some(project_id), &serde_json::json!({"path": canon.to_string_lossy(), "isObsidianVault": vault::is_vault(&canon)}));
            Ok(())
        }
    }
}

/// Where FORGE-created pages go: the vault's `FORGE/` folder when write access was granted, else the managed folder.
fn create_target(ctx: Ctx, project_id: &str) -> Result<(RootInfo, String)> {
    let rs = roots(ctx, project_id)?;
    if let Some(v) = rs.iter().find(|r| r.kind == "vault" && r.writable) {
        return Ok((v.clone(), "FORGE/pages".to_string()));
    }
    Ok((rs.into_iter().find(|r| r.kind == "managed").expect("managed root always exists"), "pages".to_string()))
}

// ---- scanning ------------------------------------------------------------------------------------------

fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<PathBuf>, report: &mut ScanReport) {
    if depth > MAX_DEPTH || out.len() >= MAX_SCAN_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(meta) = std::fs::symlink_metadata(e.path()) else { continue };
        if meta.is_dir() {
            if vault::scannable_dir(&name, &meta) {
                walk(root, &e.path(), depth + 1, out, report);
            }
        } else if name.to_lowercase().ends_with(".md") {
            if vault::is_reparse_meta(&meta) {
                report.skipped.push(format!("{}: link", e.path().strip_prefix(root).unwrap_or(&e.path()).display()));
            } else if out.len() < MAX_SCAN_FILES {
                out.push(e.path());
            } else {
                report.warnings.push(format!("scan limit of {MAX_SCAN_FILES} files reached"));
                return;
            }
        }
    }
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Reconcile the index with the files on disk (incremental: size+mtime are only hints, content hash is the truth).
pub fn scan(ctx: Ctx, project_id: &str) -> Result<ScanReport> {
    let mut report = ScanReport::default();
    for root in roots(ctx, project_id)? {
        let mut files = vec![];
        walk(&root.path, &root.path, 0, &mut files, &mut report);
        let mut present = HashSet::new();
        for f in files {
            let rel = rel_of(&root.path, &f);
            // Generated catalogue files at the top of the managed root are outputs, not knowledge.
            if root.kind == "managed" && (rel == "index.md" || rel == "log.md") {
                continue;
            }
            present.insert(rel.clone());
            if sensitive_path(&rel) {
                report.skipped.push(format!("{rel}: looks sensitive; not ingested"));
                continue;
            }
            let real = match safe_file(&root.path, &rel) {
                Ok(r) => r,
                Err(e) => {
                    report.skipped.push(format!("{rel}: {e}"));
                    continue;
                }
            };
            let Ok(meta) = std::fs::metadata(&real) else { continue };
            if meta.len() as usize > page::MAX_PAGE_BYTES {
                report.skipped.push(format!("{rel}: larger than {} KB", page::MAX_PAGE_BYTES / 1024));
                continue;
            }
            let (size, mt) = (meta.len() as i64, mtime_ms(&meta));
            if let Some((s, m, _)) = ctx.store.k_signature(project_id, root.kind, &rel)? {
                if s == size && m == mt {
                    report.unchanged += 1;
                    continue;
                }
            }
            let text = String::from_utf8_lossy(&std::fs::read(&real)?).into_owned();
            let sha = page::content_hash(&text);
            let parsed = page::parse(&text);
            for w in &parsed.warnings {
                report.warnings.push(format!("{rel}: {w}"));
            }
            let (pg, changed) = ctx.store.k_index_page(&IndexInput {
                project_id, root_kind: root.kind, rel_path: &rel, parsed: &parsed, sha256: &sha, size, mtime: mt,
                origin: "imported", author: Author::User, kind_default: "note", new_id: None,
            })?;
            report.indexed += 1;
            if changed {
                report.changed_pages.push(pg.id);
            }
        }
        report.removed += ctx.store.k_mark_missing(project_id, root.kind, &present)?;
    }
    ctx.store.k_resolve_links(project_id)?;
    regenerate_catalogue(ctx, project_id)?;
    Ok(report)
}

/// Re-read one page's file and update the index (used before trusting content and after our own writes).
pub fn reindex_page(ctx: Ctx, page: &KPage) -> Result<KPage> {
    let root = root_for(ctx, &page.project_id, &page.root_kind)?;
    let real = safe_file(&root.path, &page.rel_path)?;
    if !real.exists() {
        ctx.store.k_mark_missing(&page.project_id, &page.root_kind, &HashSet::new()).ok();
        return ctx.store.k_get(&page.id);
    }
    let text = String::from_utf8_lossy(&std::fs::read(&real)?).into_owned();
    let meta = std::fs::metadata(&real)?;
    let parsed = page::parse(&text);
    let (pg, _) = ctx.store.k_index_page(&IndexInput {
        project_id: &page.project_id, root_kind: &page.root_kind, rel_path: &page.rel_path, parsed: &parsed, sha256: &page::content_hash(&text),
        size: meta.len() as i64, mtime: mtime_ms(&meta), origin: &page.origin, author: Author::parse(&page.author).unwrap_or(Author::User), kind_default: &page.kind, new_id: None,
    })?;
    Ok(pg)
}

pub fn read_page_text(ctx: Ctx, page: &KPage) -> Result<String> {
    let root = root_for(ctx, &page.project_id, &page.root_kind)?;
    let real = safe_file(&root.path, &page.rel_path)?;
    Ok(String::from_utf8_lossy(&std::fs::read(&real)?).into_owned())
}

// ---- creation --------------------------------------------------------------------------------------------

fn clean_title(t: &str) -> Result<String> {
    let t = redact::redact_str(t.trim());
    if t.is_empty() || t.chars().count() > 200 {
        return Err(ForgeError::Other("a page needs a title of 1-200 characters".into()));
    }
    Ok(t.replace(['\n', '\r'], " "))
}

fn unique_rel(ctx: Ctx, project_id: &str, root: &RootInfo, dir: &str, slug: &str) -> Result<String> {
    for n in 1..1000 {
        let name = if n == 1 { format!("{slug}.md") } else { format!("{slug}-{n}.md") };
        let rel = format!("{dir}/{name}");
        let taken_on_disk = root.path.join(&rel).exists();
        let taken_in_index = ctx.store.k_find_by_path(project_id, root.kind, &rel)?.is_some();
        if !taken_on_disk && !taken_in_index {
            return Ok(rel);
        }
    }
    Err(ForgeError::Other("could not find a free page name".into()))
}

/// Create a page file (always `proposed`), index it, and record its sources. FORGE-created pages never start trusted.
pub fn create_page(ctx: Ctx, project_id: &str, repo_root: &Path, np: NewPage) -> Result<KPage> {
    if !KINDS.contains(&np.kind.as_str()) {
        return Err(ForgeError::Other(format!("unknown page kind \"{}\"; use one of {}", np.kind, KINDS.join(", "))));
    }
    if np.body.len() > page::MAX_PAGE_BYTES - 4096 {
        return Err(ForgeError::Other("the page body is too large".into()));
    }
    let title = clean_title(&np.title)?;
    let body = redact::redact_str(&np.body);
    let (root, dir) = create_target(ctx, project_id)?;
    let _lock = writer_lock(&root.path)?;
    let rel = unique_rel(ctx, project_id, &root, &dir, &page::slugify(&title))?;
    let real = safe_file(&root.path, &rel)?;

    // Capture sources first so a failure leaves no half-written page behind.
    let mut sources = vec![];
    for s in &np.sources {
        sources.push(capture_source(ctx, project_id, repo_root, s)?);
    }
    let id = new_id();
    let t = now();
    let meta = Meta {
        id: Some(id.clone()), title: Some(title.clone()), kind: Some(np.kind.clone()), scope: Some("project".into()),
        author: Some(np.author.as_str().into()), created: Some(t.clone()), updated: Some(t.clone()), topic: np.topic.clone(),
        sources: sources.iter().map(|s| format!("{}:{}", s.kind, s.locator)).collect(),
        files: np.files.iter().filter(|f| !f.contains("..") && !sensitive_path(f)).cloned().collect(),
        tags: np.tags.clone(),
        // `state` is deliberately NOT written: trust lives in FORGE's review records, never in the file.
        ..Default::default()
    };
    let text = page::serialize(&meta, &body);
    let sha = page::content_hash(&text);
    let op = ctx.store.k_op_begin(&id, "create", None, &sha)?;
    if let Err(e) = page::atomic_write(&real, text.as_bytes()) {
        ctx.store.k_op_finish(&op, false)?;
        return Err(e);
    }
    let fmeta = std::fs::metadata(&real)?;
    let parsed = page::parse(&text);
    let (pg, _) = ctx.store.k_index_page(&IndexInput {
        project_id, root_kind: root.kind, rel_path: &rel, parsed: &parsed, sha256: &sha, size: fmeta.len() as i64, mtime: mtime_ms(&fmeta),
        origin: "forge", author: np.author, kind_default: &np.kind, new_id: Some(&id),
    })?;
    ctx.store.k_op_finish(&op, true)?;
    for s in &sources {
        ctx.store.k_link_source(&pg.id, &s.id)?;
    }
    ctx.store.k_resolve_links(project_id)?;
    drop(_lock);
    regenerate_catalogue(ctx, project_id)?;
    ctx.store.audit(if np.author == Author::User { "user" } else { "system" }, "knowledge.create", Some(&pg.id), &serde_json::json!({"kind": np.kind, "author": np.author.as_str()}));
    ctx.store.k_get(&pg.id)
}

// ---- revisions ----------------------------------------------------------------------------------------------

/// Propose new full text for an existing page. Never touches the file.
pub fn propose_revision(ctx: Ctx, page_id: &str, new_text: &str, author: Author, note: Option<&str>) -> Result<KRevision> {
    let pg = ctx.store.k_get(page_id)?;
    if new_text.len() > page::MAX_PAGE_BYTES {
        return Err(ForgeError::Other("the revised page is too large".into()));
    }
    let text = redact::redact_str(new_text);
    // Re-read the live file so the base hash is what is on disk right now, not what we last indexed.
    let current = reindex_page(ctx, &pg)?;
    if current.missing {
        return Err(ForgeError::InvalidState("the page file is missing".into()));
    }
    ctx.store.k_add_revision(page_id, &current.current_sha256, &text, author.as_str(), note)
}

/// Replace the page file with a proposed revision. Refused (and recorded as a conflict) if the file changed since the
/// revision's base. Vault pages need write access to have been granted.
pub fn apply_revision(ctx: Ctx, revision_id: &str) -> Result<KPage> {
    let rev = ctx.store.k_revision(revision_id)?;
    if rev.state != "proposed" && rev.state != "conflict" {
        return Err(ForgeError::InvalidState(format!("this revision is already {}", rev.state)));
    }
    let pg = ctx.store.k_get(&rev.page_id)?;
    let root = root_for(ctx, &pg.project_id, &pg.root_kind)?;
    if !root.writable {
        return Err(ForgeError::InvalidState("this page lives in a read-only vault; grant FORGE write access to that vault to apply changes".into()));
    }
    let _lock = writer_lock(&root.path)?;
    let real = safe_file(&root.path, &pg.rel_path)?;
    let cur_text = String::from_utf8_lossy(&std::fs::read(&real).map_err(|e| ForgeError::Other(format!("cannot read the page: {e}")))?).into_owned();
    let cur_sha = page::content_hash(&cur_text);
    if cur_sha != rev.base_sha256 {
        ctx.store.k_set_revision_state(revision_id, "conflict")?;
        return Err(ForgeError::InvalidState("the page was edited after this revision was proposed (for example in Obsidian); nothing was overwritten. Review the current version and propose again.".into()));
    }
    let new_sha = page::content_hash(&rev.text);
    let op = ctx.store.k_op_begin(&pg.id, "apply_revision", Some(&cur_sha), &new_sha)?;
    if let Err(e) = page::atomic_write(&real, rev.text.as_bytes()) {
        ctx.store.k_op_finish(&op, false)?;
        return Err(e);
    }
    ctx.store.k_set_revision_state(revision_id, "applied")?;
    let out = reindex_page(ctx, &pg)?;
    ctx.store.k_op_finish(&op, true)?;
    ctx.store.audit("user", "knowledge.apply_revision", Some(&pg.id), &serde_json::json!({"revision": revision_id, "author": rev.author}));
    drop(_lock);
    regenerate_catalogue(ctx, &pg.project_id)?;
    Ok(out)
}

pub fn reject_revision(ctx: Ctx, revision_id: &str) -> Result<()> {
    ctx.store.k_set_revision_state(revision_id, "rejected")
}

// ---- delete -------------------------------------------------------------------------------------------------------

/// Delete a page: its file (when writable), index rows, links, revisions and quoted snapshots.
pub fn forget_page(ctx: Ctx, page_id: &str) -> Result<()> {
    let pg = ctx.store.k_get(page_id)?;
    let root = root_for(ctx, &pg.project_id, &pg.root_kind)?;
    if root.writable {
        let _lock = writer_lock(&root.path)?;
        if let Ok(real) = safe_file(&root.path, &pg.rel_path) {
            if real.exists() {
                std::fs::remove_file(&real)?;
            }
        }
    }
    ctx.store.k_forget(page_id)?;
    ctx.store.audit("user", "knowledge.forget", Some(page_id), &serde_json::json!({"title": pg.title, "fileDeleted": root.writable}));
    regenerate_catalogue(ctx, &pg.project_id)?;
    Ok(())
}

// ---- sources ---------------------------------------------------------------------------------------------------------

fn git_show(repo: &Path, spec: &str) -> Option<Vec<u8>> {
    git::run_raw(repo, &["show", spec]).ok().filter(|o| o.status.success()).map(|o| o.stdout)
}

/// Capture a source with enough to verify it later: locator, git blob id, raw and normalised hashes, a redacted snapshot.
pub fn capture_source(ctx: Ctx, project_id: &str, repo_root: &Path, spec: &SourceSpec) -> Result<KSource> {
    match spec {
        SourceSpec::RepoFile { rel } => {
            let rel = rel.replace('\\', "/");
            if rel.contains("..") || rel.starts_with('/') || sensitive_path(&rel) {
                return Err(ForgeError::PathRejected(format!("{rel} cannot be used as a knowledge source")));
            }
            let blob = git::run(repo_root, &["rev-parse", &format!("HEAD:{rel}")]).map_err(|_| ForgeError::NotFound(format!("{rel} is not a tracked file at HEAD")))?;
            let bytes = git_show(repo_root, &format!("HEAD:{rel}")).ok_or_else(|| ForgeError::NotFound(format!("{rel} is not readable at HEAD")))?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let snap: String = redact::redact_str(&text).chars().take(SNAPSHOT_BYTES).collect();
            ctx.store.k_add_source(project_id, "repo_file", &rel, Some(blob.trim()), Some(&crate::proof::sha256_hex(&bytes)), Some(&page::content_hash(&text)), Some(&snap))
        }
        SourceSpec::Commit { sha } => {
            let full = git::run(repo_root, &["rev-parse", "--verify", &format!("{sha}^{{commit}}")]).map_err(|_| ForgeError::NotFound(format!("commit {sha}")))?;
            ctx.store.k_add_source(project_id, "commit", full.trim(), Some(full.trim()), None, None, None)
        }
        SourceSpec::Mission { id, event_seq } => {
            ctx.store.get_mission(id)?;
            let loc = match event_seq {
                Some(s) => format!("{id}#event:{s}"),
                None => id.clone(),
            };
            ctx.store.k_add_source(project_id, "mission", &loc, None, None, None, None)
        }
        SourceSpec::CheckRun { id } => {
            let run = ctx.store.get_run(id)?;
            ctx.store.k_add_source(project_id, "check_run", id, None, Some(&run.fingerprint), None, None)
        }
        SourceSpec::UserNote { text } => {
            let t = redact::redact_str(text.trim());
            if t.is_empty() {
                return Err(ForgeError::Other("an empty note is not a source".into()));
            }
            ctx.store.k_add_source(project_id, "user_note", &redact::truncate(&t, 200).0, None, Some(&crate::proof::sha256_hex(t.as_bytes())), None, Some(&t))
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessReport {
    pub checked: usize,
    pub changed: usize,
    pub missing: usize,
}

/// Compare captured repo-file sources with the project's current HEAD. Changed/missing sources make pages stale.
pub fn refresh_sources(ctx: Ctx, project_id: &str, repo_root: &Path) -> Result<FreshnessReport> {
    let mut rep = FreshnessReport::default();
    let sources: Vec<KSource> = {
        let c = ctx.store.c();
        let mut s = c.prepare("SELECT id, project_id, kind, locator, git_blob, raw_sha256, norm_sha256, snapshot, extractor, state, captured_at, checked_at FROM knowledge_source WHERE project_id=?1 AND kind='repo_file'")?;
        s.query_map([project_id], |r| {
            Ok(KSource { id: r.get(0)?, project_id: r.get(1)?, kind: r.get(2)?, locator: r.get(3)?, git_blob: r.get(4)?, raw_sha256: r.get(5)?, norm_sha256: r.get(6)?, snapshot: r.get(7)?, extractor: r.get(8)?, state: r.get(9)?, captured_at: r.get(10)?, checked_at: r.get(11)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for s in sources {
        rep.checked += 1;
        let now_blob = git::run(repo_root, &["rev-parse", &format!("HEAD:{}", s.locator)]).ok().map(|b| b.trim().to_string());
        let state = match (&now_blob, &s.git_blob) {
            (None, _) => {
                rep.missing += 1;
                "missing"
            }
            (Some(n), Some(o)) if n == o => "current",
            (Some(_), Some(_)) => {
                rep.changed += 1;
                "changed"
            }
            _ => "unknown",
        };
        ctx.store.k_set_source_state(&s.id, state)?;
    }
    Ok(rep)
}

// ---- generated catalogue -----------------------------------------------------------------------------------------------

/// `index.md` (catalogue) and `log.md` (history) in the managed root, generated from the database.
pub fn regenerate_catalogue(ctx: Ctx, project_id: &str) -> Result<()> {
    let root = root_for(ctx, project_id, "managed")?;
    let pages = ctx.store.k_list(project_id)?;
    let mut idx = String::from("# Knowledge index\n\n_Generated by FORGE from its database. Do not edit; changes are overwritten._\n\n");
    for kind in KINDS {
        let of: Vec<&KPage> = pages.iter().filter(|p| p.kind == *kind && !p.missing).collect();
        if of.is_empty() {
            continue;
        }
        idx.push_str(&format!("## {}\n\n", kind.replace('_', " ")));
        for p in of {
            idx.push_str(&format!("- [[{}]] — {} ({})\n", p.title, p.effective_state, p.author));
        }
        idx.push('\n');
    }
    let mut log = String::from("# Knowledge log\n\n_Generated by FORGE from its review history._\n\n");
    let rows: Vec<(String, String, String, Option<String>, String)> = {
        let c = ctx.store.c();
        let mut s = c.prepare(
            "SELECT r.at, r.action, COALESCE(p.title, r.page_id), r.note, r.reviewer FROM knowledge_review r LEFT JOIN knowledge_page p ON p.id = r.page_id WHERE r.project_id=?1 ORDER BY r.id DESC LIMIT 500",
        )?;
        s.query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (at, action, title, note, who) in rows {
        log.push_str(&format!("## [{}] {} | {}\n", &at[..at.len().min(10)], action, title));
        log.push_str(&format!("by {who}{}\n\n", note.map(|n| format!(" — {n}")).unwrap_or_default()));
    }
    let _lock = writer_lock(&root.path)?;
    page::atomic_write(&root.path.join("index.md"), idx.as_bytes())?;
    page::atomic_write(&root.path.join("log.md"), log.as_bytes())?;
    Ok(())
}

// ---- crash recovery ------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryNotes {
    pub completed: usize,
    pub rolled_back: usize,
    pub conflicts: usize,
    pub temp_files_removed: usize,
}

/// Resolve operations that were interrupted mid-write by inspecting what is actually on disk.
pub fn recover(ctx: Ctx) -> Result<RecoveryNotes> {
    let mut notes = RecoveryNotes::default();
    for (op, page_id, _kind, old, new) in ctx.store.k_ops_unfinished()? {
        let Ok(pg) = ctx.store.k_get(&page_id) else {
            ctx.store.k_op_finish(&op, false)?;
            continue;
        };
        let actual = read_page_text(ctx, &pg).ok().map(|t| page::content_hash(&t));
        match (&actual, &old, &new) {
            (Some(a), _, Some(n)) if a == n => {
                reindex_page(ctx, &pg)?;
                ctx.store.k_op_finish(&op, true)?;
                notes.completed += 1;
            }
            (Some(a), Some(o), _) if a == o => {
                ctx.store.k_op_finish(&op, false)?;
                notes.rolled_back += 1;
            }
            _ => {
                ctx.store.k_op_finish(&op, false)?;
                notes.conflicts += 1;
            }
        }
    }
    // Leftover temp files from a crash between create and rename.
    let projects = ctx.store.list_projects()?;
    for p in projects {
        for r in roots(ctx, &p.id)? {
            if let Ok(rd) = std::fs::read_dir(r.path.join("pages")) {
                for e in rd.flatten() {
                    if e.file_name().to_string_lossy().starts_with(".forge-tmp-") && std::fs::remove_file(e.path()).is_ok() {
                        notes.temp_files_removed += 1;
                    }
                }
            }
        }
    }
    Ok(notes)
}
