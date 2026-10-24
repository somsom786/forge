//! Knowledge persistence. SQLite is authoritative for *trust* (status, verified revision, review history); the page
//! files are authoritative for *content*. A page is "verified" only while its current content hash equals the hash a
//! human verified; any edit makes it unreviewed again, and nothing written inside a page can grant verification.

use super::page::{self, Author, PageState, Parsed};
use crate::error::{ForgeError, Result};
use crate::store::{Store, new_id, now};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KPage {
    pub id: String,
    pub project_id: String,
    /// `managed` (FORGE-owned directory) or `vault` (a connected Obsidian vault folder).
    pub root_kind: String,
    pub rel_path: String,
    pub slug: String,
    pub title: String,
    pub kind: String,
    pub scope: String,
    pub author: String,
    /// `forge` (created by FORGE) or `imported` (found in a folder FORGE was pointed at).
    pub origin: String,
    /// What a human last decided.
    pub status: String,
    /// What is true right now: a verified page whose content changed since is `proposed` again.
    pub effective_state: String,
    pub modified_since_verified: bool,
    pub verified_sha256: Option<String>,
    pub verified_at: Option<String>,
    pub verified_note: Option<String>,
    pub current_sha256: String,
    pub topic: Option<String>,
    pub supersedes_id: Option<String>,
    pub superseded_by_id: Option<String>,
    pub files: Vec<String>,
    pub tags: Vec<String>,
    pub warnings: Vec<String>,
    pub missing: bool,
    pub created_at: String,
    pub updated_at: String,
    pub indexed_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KReview {
    pub action: String,
    pub revision_sha256: Option<String>,
    pub reviewer: String,
    pub note: Option<String>,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KSource {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub locator: String,
    pub git_blob: Option<String>,
    pub raw_sha256: Option<String>,
    pub norm_sha256: Option<String>,
    pub snapshot: Option<String>,
    pub extractor: String,
    /// current | changed | missing | unknown
    pub state: String,
    pub captured_at: String,
    pub checked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KLink {
    pub kind: String,
    pub target: String,
    pub to_page: Option<String>,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KRevision {
    pub id: String,
    pub page_id: String,
    pub base_sha256: String,
    pub text: String,
    pub author: String,
    pub note: Option<String>,
    pub state: String,
    pub created_at: String,
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackageRow {
    pub id: String,
    pub mission_id: String,
    pub created_at: String,
    pub query: String,
    pub params: serde_json::Value,
    pub items: serde_json::Value,
    pub omitted: serde_json::Value,
    pub injected_text: String,
    pub injected_sha256: String,
    pub char_budget: i64,
}

const PAGE_COLS: &str = "id, project_id, root_kind, rel_path, slug, title, kind, scope, author, origin, status, verified_sha256, verified_at, verified_note, current_sha256, topic, supersedes_id, superseded_by_id, files_json, tags_json, warnings_json, missing, created_at, updated_at, indexed_at";

fn page_from(r: &Row) -> rusqlite::Result<KPage> {
    let status: String = r.get(10)?;
    let verified_sha: Option<String> = r.get(11)?;
    let current: String = r.get(14)?;
    let modified = status == "verified" && verified_sha.as_deref() != Some(current.as_str());
    let files: String = r.get(18)?;
    let tags: String = r.get(19)?;
    let warnings: String = r.get(20)?;
    Ok(KPage {
        id: r.get(0)?,
        project_id: r.get(1)?,
        root_kind: r.get(2)?,
        rel_path: r.get(3)?,
        slug: r.get(4)?,
        title: r.get(5)?,
        kind: r.get(6)?,
        scope: r.get(7)?,
        author: r.get(8)?,
        origin: r.get(9)?,
        effective_state: if modified { "proposed".into() } else { status.clone() },
        modified_since_verified: modified,
        status,
        verified_sha256: verified_sha,
        verified_at: r.get(12)?,
        verified_note: r.get(13)?,
        current_sha256: current,
        topic: r.get(15)?,
        supersedes_id: r.get(16)?,
        superseded_by_id: r.get(17)?,
        files: serde_json::from_str(&files).unwrap_or_default(),
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        warnings: serde_json::from_str(&warnings).unwrap_or_default(),
        missing: r.get::<_, i64>(21)? != 0,
        created_at: r.get(22)?,
        updated_at: r.get(23)?,
        indexed_at: r.get(24)?,
    })
}

pub struct IndexInput<'a> {
    pub project_id: &'a str,
    pub root_kind: &'a str,
    pub rel_path: &'a str,
    pub parsed: &'a Parsed,
    pub sha256: &'a str,
    pub size: i64,
    pub mtime: i64,
    /// Who/what created the file: used only when the page is new to the index.
    pub origin: &'a str,
    pub author: Author,
    pub kind_default: &'a str,
    /// Id to use when the page is new (FORGE-created pages carry the id they wrote into the file).
    pub new_id: Option<&'a str>,
}

fn log_review(tx: &Transaction, page_id: &str, project_id: &str, action: &str, sha: Option<&str>, reviewer: &str, note: Option<&str>) -> Result<()> {
    tx.execute(
        "INSERT INTO knowledge_review(page_id, project_id, action, revision_sha256, reviewer, note, at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![page_id, project_id, action, sha, reviewer, note, now()],
    )?;
    Ok(())
}

fn fts_replace(tx: &Transaction, page_id: &str, project_id: &str, title: &str, body: &str, files: &[String], tags: &[String]) -> Result<()> {
    tx.execute("DELETE FROM knowledge_fts WHERE page_id=?1", [page_id])?;
    tx.execute(
        "INSERT INTO knowledge_fts(page_id, project_id, title, body, files, tags) VALUES (?1,?2,?3,?4,?5,?6)",
        params![page_id, project_id, title, super::query::index_text(body), files.join(" "), tags.join(" ")],
    )?;
    Ok(())
}

fn write_links(tx: &Transaction, page_id: &str, body: &str, files: &[String]) -> Result<()> {
    tx.execute("DELETE FROM knowledge_link WHERE from_page=?1", [page_id])?;
    let mut seen = HashSet::new();
    for l in page::wikilinks(body) {
        if seen.insert(("wikilink", l.target.clone())) {
            tx.execute("INSERT OR IGNORE INTO knowledge_link(from_page, kind, target) VALUES (?1,'wikilink',?2)", params![page_id, l.target])?;
        }
    }
    for f in files {
        if seen.insert(("file", f.clone())) {
            tx.execute("INSERT OR IGNORE INTO knowledge_link(from_page, kind, target) VALUES (?1,'file',?2)", params![page_id, f])?;
        }
    }
    Ok(())
}

impl Store {
    /// Insert or update the index row for a page file, together with its FTS row and links, in one transaction.
    /// Trust fields are never taken from the file: a new page starts `proposed`; an existing page keeps its status.
    pub fn k_index_page(&self, i: &IndexInput) -> Result<(KPage, bool)> {
        let meta = &i.parsed.meta;
        let title = meta.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
            i.rel_path.rsplit('/').next().unwrap_or(i.rel_path).trim_end_matches(".md").replace(['-', '_'], " ")
        });
        let kind = meta.kind.clone().filter(|k| !k.is_empty()).unwrap_or_else(|| i.kind_default.to_string());
        let scope = if meta.scope.as_deref() == Some("global") { "global" } else { "project" };
        let slug = page::slugify(i.rel_path.rsplit('/').next().unwrap_or(i.rel_path).trim_end_matches(".md"));
        let (files, tags) = (meta.files.clone(), meta.tags.clone());
        // The stored and indexed copy is redacted; the file (and its hash) stay exactly as the user wrote them.
        let body: String = crate::redact::redact_str(&i.parsed.body.chars().take(page::MAX_PAGE_BYTES).collect::<String>());
        let t = now();

        let mut c = self.c();
        let tx = c.transaction()?;
        let existing: Option<(String, String, String, Option<String>)> = tx
            .query_row(
                "SELECT id, current_sha256, status, verified_sha256 FROM knowledge_page WHERE project_id=?1 AND root_kind=?2 AND rel_path=?3",
                params![i.project_id, i.root_kind, i.rel_path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (id, changed) = match existing {
            Some((id, old_sha, status, verified)) => {
                let changed = old_sha != i.sha256;
                tx.execute(
                    "UPDATE knowledge_page SET slug=?2, title=?3, kind=?4, scope=?5, current_sha256=?6, file_size=?7, file_mtime=?8, topic=?9,
                     files_json=?10, tags_json=?11, warnings_json=?12, body_text=?13, missing=0, indexed_at=?14,
                     updated_at=CASE WHEN ?15 THEN ?14 ELSE updated_at END WHERE id=?1",
                    params![id, slug, title, kind, scope, i.sha256, i.size, i.mtime, meta.topic, serde_json::to_string(&files)?, serde_json::to_string(&tags)?, serde_json::to_string(&i.parsed.warnings)?, body, t, changed],
                )?;
                if changed && status == "verified" && verified.as_deref() != Some(i.sha256) {
                    log_review(&tx, &id, i.project_id, "content_changed", Some(i.sha256), "system", Some("content changed after verification; the page is unreviewed again"))?;
                }
                (id, changed)
            }
            None => {
                let id = i.new_id.map(str::to_string).unwrap_or_else(new_id);
                tx.execute(
                    "INSERT INTO knowledge_page(id, project_id, root_kind, rel_path, slug, title, kind, scope, author, origin, status, current_sha256, file_size, file_mtime,
                     topic, files_json, tags_json, warnings_json, body_text, created_at, updated_at, indexed_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'proposed',?11,?12,?13,?14,?15,?16,?17,?18,?19,?19,?19)",
                    params![id, i.project_id, i.root_kind, i.rel_path, slug, title, kind, scope, i.author.as_str(), i.origin, i.sha256, i.size, i.mtime, meta.topic, serde_json::to_string(&files)?, serde_json::to_string(&tags)?, serde_json::to_string(&i.parsed.warnings)?, body, t],
                )?;
                log_review(&tx, &id, i.project_id, "created", Some(i.sha256), if i.origin == "forge" { "system" } else { "import" }, None)?;
                (id, true)
            }
        };
        fts_replace(&tx, &id, i.project_id, &title, &body, &files, &tags)?;
        write_links(&tx, &id, &body, &files)?;
        tx.commit()?;
        drop(c);
        Ok((self.k_get(&id)?, changed))
    }

    pub fn k_get(&self, id: &str) -> Result<KPage> {
        self.c()
            .query_row(&format!("SELECT {PAGE_COLS} FROM knowledge_page WHERE id=?1"), [id], page_from)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("knowledge page {id}")))
    }

    pub fn k_body(&self, id: &str) -> Result<String> {
        self.c()
            .query_row("SELECT body_text FROM knowledge_page WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("knowledge page {id}")))
    }

    pub fn k_list(&self, project_id: &str) -> Result<Vec<KPage>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {PAGE_COLS} FROM knowledge_page WHERE project_id=?1 ORDER BY title COLLATE NOCASE"))?;
        Ok(s.query_map([project_id], page_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn k_find_by_path(&self, project_id: &str, root_kind: &str, rel_path: &str) -> Result<Option<KPage>> {
        Ok(self
            .c()
            .query_row(&format!("SELECT {PAGE_COLS} FROM knowledge_page WHERE project_id=?1 AND root_kind=?2 AND rel_path=?3"), params![project_id, root_kind, rel_path], page_from)
            .optional()?)
    }

    /// (size, mtime, sha) of what is currently indexed, for cheap change detection.
    pub fn k_signature(&self, project_id: &str, root_kind: &str, rel_path: &str) -> Result<Option<(i64, i64, String, bool)>> {
        Ok(self
            .c()
            .query_row(
                "SELECT file_size, file_mtime, current_sha256, missing FROM knowledge_page WHERE project_id=?1 AND root_kind=?2 AND rel_path=?3",
                params![project_id, root_kind, rel_path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0)),
            )
            .optional()?)
    }

    /// Mark ONE page whose file is gone. (Never mark a whole root: one absent file must not hide every other page.)
    pub fn k_mark_page_missing(&self, page_id: &str) -> Result<()> {
        self.c().execute("UPDATE knowledge_page SET missing=1 WHERE id=?1", [page_id])?;
        self.c().execute("DELETE FROM knowledge_fts WHERE page_id=?1", [page_id])?;
        Ok(())
    }

    /// Full-text search restricted to one project. Lower bm25 = better; returned as a positive relevance.
    pub fn k_search(&self, project_id: &str, fts_query: &str, limit: i64) -> Result<Vec<(KPage, f64)>> {
        let c = self.c();
        let mut s = c.prepare(&format!(
            "SELECT {}, bm25(knowledge_fts, 0.0, 0.0, 6.0, 1.0, 3.0, 2.0) AS rank
             FROM knowledge_fts JOIN knowledge_page p ON p.id = knowledge_fts.page_id
             WHERE knowledge_fts MATCH ?1 AND knowledge_fts.project_id = ?2 AND p.missing = 0
             ORDER BY rank LIMIT ?3",
            PAGE_COLS.split(", ").map(|c| format!("p.{c}")).collect::<Vec<_>>().join(", ")
        ))?;
        let rows = s.query_map(params![fts_query, project_id, limit], |r| {
            let page = page_from(r)?;
            let rank: f64 = r.get(25)?;
            Ok((page, -rank))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Mark pages whose file is no longer present (they stay on record but are never retrieved).
    pub fn k_mark_missing(&self, project_id: &str, root_kind: &str, present: &HashSet<String>) -> Result<usize> {
        let ids: Vec<(String, String)> = {
            let c = self.c();
            let mut s = c.prepare("SELECT id, rel_path FROM knowledge_page WHERE project_id=?1 AND root_kind=?2 AND missing=0")?;
            s.query_map(params![project_id, root_kind], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut n = 0;
        for (id, rel) in ids {
            if !present.contains(&rel) {
                self.c().execute("UPDATE knowledge_page SET missing=1 WHERE id=?1", [&id])?;
                self.c().execute("DELETE FROM knowledge_fts WHERE page_id=?1", [&id])?;
                n += 1;
            }
        }
        Ok(n)
    }

    // ---- trust decisions (the only way a page becomes verified) -------------------------------------

    /// Bind verification to the exact revision the reviewer saw. Fails if the content changed in the meantime.
    pub fn k_verify(&self, page_id: &str, expected_sha256: &str, note: Option<&str>) -> Result<KPage> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let (project_id, status, current, missing): (String, String, String, i64) = tx
            .query_row("SELECT project_id, status, current_sha256, missing FROM knowledge_page WHERE id=?1", [page_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("knowledge page {page_id}")))?;
        if missing != 0 {
            return Err(ForgeError::InvalidState("this page's file is missing; it cannot be verified".into()));
        }
        if matches!(status.as_str(), "archived" | "superseded") {
            return Err(ForgeError::InvalidState(format!("a {status} page cannot be verified; restore it first")));
        }
        if current != expected_sha256 {
            return Err(ForgeError::InvalidState("the page changed since you opened it; review the new version before verifying".into()));
        }
        tx.execute("UPDATE knowledge_page SET status='verified', verified_sha256=?2, verified_at=?3, verified_note=?4 WHERE id=?1", params![page_id, current, now(), note])?;
        log_review(&tx, page_id, &project_id, "verify", Some(&current), "user", note)?;
        tx.commit()?;
        drop(c);
        self.k_get(page_id)
    }

    /// dispute | archive | restore. Restoring returns a page to `proposed`: it must be verified again.
    pub fn k_set_status(&self, page_id: &str, action: &str, note: Option<&str>) -> Result<KPage> {
        let (new_status, clear_verification) = match action {
            "dispute" => (PageState::Disputed, true),
            "archive" => (PageState::Archived, true),
            "restore" => (PageState::Proposed, true),
            other => return Err(ForgeError::Other(format!("unknown status action \"{other}\""))),
        };
        let mut c = self.c();
        let tx = c.transaction()?;
        let (project_id, status, current): (String, String, String) = tx
            .query_row("SELECT project_id, status, current_sha256 FROM knowledge_page WHERE id=?1", [page_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("knowledge page {page_id}")))?;
        if action == "restore" && !matches!(status.as_str(), "archived" | "disputed" | "superseded") {
            return Err(ForgeError::InvalidState("only an archived, disputed or superseded page can be restored".into()));
        }
        if action == "restore" && status == "superseded" {
            tx.execute("UPDATE knowledge_page SET superseded_by_id=NULL WHERE id=?1", [page_id])?;
        }
        tx.execute(
            if clear_verification { "UPDATE knowledge_page SET status=?2, verified_sha256=NULL, verified_at=NULL, verified_note=NULL WHERE id=?1" } else { "UPDATE knowledge_page SET status=?2 WHERE id=?1" },
            params![page_id, new_status.as_str()],
        )?;
        log_review(&tx, page_id, &project_id, action, Some(&current), "user", note)?;
        tx.commit()?;
        drop(c);
        self.k_get(page_id)
    }

    /// `old` is replaced by `new`. Never transfers verification; rejects self-links and cycles atomically.
    pub fn k_supersede(&self, old_id: &str, new_id: &str, note: Option<&str>) -> Result<KPage> {
        if old_id == new_id {
            return Err(ForgeError::InvalidState("a page cannot supersede itself".into()));
        }
        let mut c = self.c();
        let tx = c.transaction()?;
        let get = |id: &str| -> Result<(String, String, Option<String>)> {
            tx.query_row("SELECT project_id, status, superseded_by_id FROM knowledge_page WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?
                .ok_or_else(|| ForgeError::NotFound(format!("knowledge page {id}")))
        };
        let (p_old, st_old, _) = get(old_id)?;
        let (p_new, st_new, _) = get(new_id)?;
        if p_old != p_new {
            return Err(ForgeError::InvalidState("pages belong to different projects".into()));
        }
        if matches!(st_new.as_str(), "archived" | "superseded" | "disputed") {
            return Err(ForgeError::InvalidState(format!("the replacement is {st_new}; choose a live page")));
        }
        if st_old == "superseded" {
            return Err(ForgeError::InvalidState("this page is already superseded".into()));
        }
        // Walk forward from the replacement: if the chain ever reaches `old`, this would close a cycle.
        let mut cur = Some(new_id.to_string());
        let mut hops = 0;
        while let Some(id) = cur {
            if id == old_id {
                return Err(ForgeError::InvalidState("that would create a supersession cycle".into()));
            }
            hops += 1;
            if hops > 1000 {
                return Err(ForgeError::InvalidState("supersession chain is unreasonably long".into()));
            }
            cur = get(&id)?.2;
        }
        tx.execute("UPDATE knowledge_page SET status='superseded', superseded_by_id=?2, verified_sha256=NULL, verified_at=NULL, verified_note=NULL WHERE id=?1", params![old_id, new_id])?;
        tx.execute("UPDATE knowledge_page SET supersedes_id=?2 WHERE id=?1", params![new_id, old_id])?;
        log_review(&tx, old_id, &p_old, "supersede", None, "user", note.or(Some(&format!("replaced by {new_id}"))))?;
        tx.commit()?;
        drop(c);
        self.k_get(old_id)
    }

    pub fn k_reviews(&self, page_id: &str) -> Result<Vec<KReview>> {
        let c = self.c();
        let mut s = c.prepare("SELECT action, revision_sha256, reviewer, note, at FROM knowledge_review WHERE page_id=?1 ORDER BY id")?;
        Ok(s.query_map([page_id], |r| Ok(KReview { action: r.get(0)?, revision_sha256: r.get(1)?, reviewer: r.get(2)?, note: r.get(3)?, at: r.get(4)? }))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- links ---------------------------------------------------------------------------------------

    /// Resolve wikilinks to pages by file name, vault path or title. Ambiguity is recorded, never guessed.
    pub fn k_resolve_links(&self, project_id: &str) -> Result<()> {
        let pages = self.k_list(project_id)?;
        let mut c = self.c();
        let tx = c.transaction()?;
        let rows: Vec<(String, String)> = {
            let mut s = tx.prepare("SELECT l.from_page, l.target FROM knowledge_link l JOIN knowledge_page p ON p.id = l.from_page WHERE p.project_id=?1 AND l.kind='wikilink'")?;
            s.query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        // Obsidian resolves a link by file name or vault path, case-insensitively; titles are FORGE's addition. Names are
        // compared composed (NFC) and case-folded, so `[[Café]]` finds a decomposed `Café.md` from macOS and `[[über]]`
        // finds `Über.md`. Slugs are never used to match: different names share them (every non-ASCII name, `C++` and
        // `C`, names that differ after 60 characters), so a slug match can point a link at the wrong page.
        let keys: Vec<(&KPage, [String; 3])> = pages
            .iter()
            .filter(|p| !p.missing)
            .map(|p| {
                let path = p.rel_path.strip_suffix(".md").unwrap_or(&p.rel_path);
                let stem = path.rsplit('/').next().unwrap_or(path);
                (p, [link_key(&p.title), link_key(stem), link_key(path)])
            })
            .collect();
        for (from, target) in rows {
            let bare = target.trim().trim_start_matches('/');
            let bare = bare.strip_suffix(".md").unwrap_or(bare);
            let t_key = link_key(bare);
            let matches: Vec<&KPage> = keys
                .iter()
                .filter(|(p, k)| p.id != from && k.contains(&t_key))
                .map(|(p, _)| *p)
                .collect();
            let (to, amb) = match matches.len() {
                0 => (None, false),
                1 => (Some(matches[0].id.clone()), false),
                _ => (None, true),
            };
            tx.execute("UPDATE knowledge_link SET to_page=?3, ambiguous=?4 WHERE from_page=?1 AND kind='wikilink' AND target=?2", params![from, target, to, amb as i64])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn k_links(&self, page_id: &str) -> Result<Vec<KLink>> {
        let c = self.c();
        let mut s = c.prepare("SELECT kind, target, to_page, ambiguous FROM knowledge_link WHERE from_page=?1 ORDER BY kind, target")?;
        Ok(s.query_map([page_id], |r| Ok(KLink { kind: r.get(0)?, target: r.get(1)?, to_page: r.get(2)?, ambiguous: r.get::<_, i64>(3)? != 0 }))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn k_backlinks(&self, page_id: &str) -> Result<Vec<KPage>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {PAGE_COLS} FROM knowledge_page WHERE id IN (SELECT from_page FROM knowledge_link WHERE to_page=?1) ORDER BY title COLLATE NOCASE"))?;
        Ok(s.query_map([page_id], page_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- sources -----------------------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn k_add_source(&self, project_id: &str, kind: &str, locator: &str, git_blob: Option<&str>, raw_sha: Option<&str>, norm_sha: Option<&str>, snapshot: Option<&str>) -> Result<KSource> {
        let id = new_id();
        let t = now();
        self.c().execute(
            "INSERT INTO knowledge_source(id, project_id, kind, locator, git_blob, raw_sha256, norm_sha256, snapshot, captured_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id, project_id, kind, locator, git_blob, raw_sha, norm_sha, snapshot, t],
        )?;
        self.k_source(&id)
    }

    pub fn k_source(&self, id: &str) -> Result<KSource> {
        self.c()
            .query_row("SELECT id, project_id, kind, locator, git_blob, raw_sha256, norm_sha256, snapshot, extractor, state, captured_at, checked_at FROM knowledge_source WHERE id=?1", [id], source_from)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("knowledge source {id}")))
    }

    pub fn k_link_source(&self, page_id: &str, source_id: &str) -> Result<()> {
        self.c().execute("INSERT OR IGNORE INTO knowledge_page_source(page_id, source_id) VALUES (?1,?2)", params![page_id, source_id])?;
        Ok(())
    }

    pub fn k_sources_for(&self, page_id: &str) -> Result<Vec<KSource>> {
        let c = self.c();
        let mut s = c.prepare("SELECT s.id, s.project_id, s.kind, s.locator, s.git_blob, s.raw_sha256, s.norm_sha256, s.snapshot, s.extractor, s.state, s.captured_at, s.checked_at FROM knowledge_source s JOIN knowledge_page_source ps ON ps.source_id = s.id WHERE ps.page_id=?1 ORDER BY s.captured_at")?;
        Ok(s.query_map([page_id], source_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn k_set_source_state(&self, id: &str, state: &str) -> Result<()> {
        self.c().execute("UPDATE knowledge_source SET state=?2, checked_at=?3 WHERE id=?1", params![id, state, now()])?;
        Ok(())
    }

    // ---- revisions (agent/user proposals against an existing page) -------------------------------------------

    pub fn k_add_revision(&self, page_id: &str, base_sha: &str, text: &str, author: &str, note: Option<&str>) -> Result<KRevision> {
        let id = new_id();
        self.c().execute(
            "INSERT INTO knowledge_revision(id, page_id, base_sha256, text, author, note, state, created_at) VALUES (?1,?2,?3,?4,?5,?6,'proposed',?7)",
            params![id, page_id, base_sha, text, author, note, now()],
        )?;
        self.k_revision(&id)
    }

    pub fn k_revision(&self, id: &str) -> Result<KRevision> {
        self.c()
            .query_row("SELECT id, page_id, base_sha256, text, author, note, state, created_at, decided_at FROM knowledge_revision WHERE id=?1", [id], revision_from)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("revision {id}")))
    }

    pub fn k_revisions(&self, page_id: &str) -> Result<Vec<KRevision>> {
        let c = self.c();
        let mut s = c.prepare("SELECT id, page_id, base_sha256, text, author, note, state, created_at, decided_at FROM knowledge_revision WHERE page_id=?1 ORDER BY created_at DESC")?;
        Ok(s.query_map([page_id], revision_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn k_set_revision_state(&self, id: &str, state: &str) -> Result<()> {
        self.c().execute("UPDATE knowledge_revision SET state=?2, decided_at=?3 WHERE id=?1 AND state IN ('proposed','conflict')", params![id, state, now()])?;
        Ok(())
    }

    // ---- write journal -------------------------------------------------------------------------------------------

    pub fn k_op_begin(&self, page_id: &str, kind: &str, old: Option<&str>, new: &str) -> Result<String> {
        let id = new_id();
        self.c().execute("INSERT INTO knowledge_op(id, page_id, kind, old_sha256, new_sha256, state, started_at) VALUES (?1,?2,?3,?4,?5,'started',?6)", params![id, page_id, kind, old, new, now()])?;
        Ok(id)
    }

    pub fn k_op_finish(&self, id: &str, ok: bool) -> Result<()> {
        self.c().execute("UPDATE knowledge_op SET state=?2, finished_at=?3 WHERE id=?1", params![id, if ok { "done" } else { "failed" }, now()])?;
        Ok(())
    }

    /// Operations that started but never finished (a crash mid-write): (id, page_id, kind, old_sha, new_sha).
    pub fn k_ops_unfinished(&self) -> Result<Vec<(String, String, String, Option<String>, Option<String>)>> {
        let c = self.c();
        let mut s = c.prepare("SELECT id, page_id, kind, old_sha256, new_sha256 FROM knowledge_op WHERE state='started'")?;
        Ok(s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- context packages ---------------------------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn k_save_context(&self, mission_id: &str, query: &str, params_v: &serde_json::Value, items: &serde_json::Value, omitted: &serde_json::Value, injected: &str, sha: &str, budget: i64) -> Result<ContextPackageRow> {
        let id = new_id();
        let at = now();
        self.c().execute(
            "INSERT INTO context_package(id, mission_id, created_at, query, params_json, items_json, omitted_json, injected_text, injected_sha256, char_budget) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![id, mission_id, at, query, params_v.to_string(), items.to_string(), omitted.to_string(), injected, sha, budget],
        )?;
        Ok(ContextPackageRow { id, mission_id: mission_id.into(), created_at: at, query: query.into(), params: params_v.clone(), items: items.clone(), omitted: omitted.clone(), injected_text: injected.into(), injected_sha256: sha.into(), char_budget: budget })
    }

    pub fn k_contexts_for(&self, mission_id: &str) -> Result<Vec<ContextPackageRow>> {
        let c = self.c();
        let mut s = c.prepare("SELECT id, mission_id, created_at, query, params_json, items_json, omitted_json, injected_text, injected_sha256, char_budget FROM context_package WHERE mission_id=?1 ORDER BY created_at DESC")?;
        Ok(s.query_map([mission_id], |r| {
            let p: String = r.get(4)?;
            let i: String = r.get(5)?;
            let o: String = r.get(6)?;
            Ok(ContextPackageRow {
                id: r.get(0)?, mission_id: r.get(1)?, created_at: r.get(2)?, query: r.get(3)?,
                params: serde_json::from_str(&p).unwrap_or_default(), items: serde_json::from_str(&i).unwrap_or_default(), omitted: serde_json::from_str(&o).unwrap_or_default(),
                injected_text: r.get(7)?, injected_sha256: r.get(8)?, char_budget: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Remove a page entirely (file deletion is the caller's job). Historical context packages keep their snapshot
    /// text for provenance but the page's body inside them is scrubbed.
    pub fn k_forget(&self, page_id: &str) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        for sql in [
            "DELETE FROM knowledge_fts WHERE page_id=?1",
            "DELETE FROM knowledge_link WHERE from_page=?1 OR to_page=?1",
            "DELETE FROM knowledge_page_source WHERE page_id=?1",
            "DELETE FROM knowledge_revision WHERE page_id=?1",
            "UPDATE knowledge_page SET supersedes_id=NULL WHERE supersedes_id=?1",
            "UPDATE knowledge_page SET superseded_by_id=NULL WHERE superseded_by_id=?1",
            "DELETE FROM knowledge_page WHERE id=?1",
        ] {
            tx.execute(sql, [page_id])?;
        }
        // Scrub the injected snapshots that quoted this page: remove the item itself (excerpt, sources, reasons), not just its id.
        let rows: Vec<(String, String)> = {
            let mut s = tx.prepare("SELECT id, items_json FROM context_package WHERE items_json LIKE ?1")?;
            s.query_map([format!("%{page_id}%")], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, items) in rows {
            let mut v: serde_json::Value = serde_json::from_str(&items).unwrap_or(serde_json::Value::Null);
            if let Some(a) = v.as_array_mut() {
                a.retain(|it| it.get("pageId").and_then(|p| p.as_str()) != Some(page_id));
            }
            tx.execute("UPDATE context_package SET injected_text='[purged: a page quoted here was deleted]', items_json=?2 WHERE id=?1", params![id, v.to_string()])?;
        }
        // Source snapshots that belonged only to this page go with it.
        tx.execute("DELETE FROM knowledge_source WHERE id NOT IN (SELECT source_id FROM knowledge_page_source)", [])?;
        tx.commit()?;
        Ok(())
    }

    pub fn k_fts_rebuild(&self, project_id: &str) -> Result<usize> {
        let pages = self.k_list(project_id)?;
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM knowledge_fts WHERE project_id=?1", [project_id])?;
        let mut n = 0;
        for p in pages.iter().filter(|p| !p.missing) {
            let body: String = tx.query_row("SELECT body_text FROM knowledge_page WHERE id=?1", [&p.id], |r| r.get(0))?;
            fts_replace(&tx, &p.id, project_id, &p.title, &body, &p.files, &p.tags)?;
            n += 1;
        }
        tx.commit()?;
        Ok(n)
    }

    pub fn k_fts_count(&self, project_id: &str) -> Result<i64> {
        Ok(self.c().query_row("SELECT count(*) FROM knowledge_fts WHERE project_id=?1", [project_id], |r| r.get(0))?)
    }

    // ---- vault configuration ----------------------------------------------------------------------------------------------

    pub fn project_vault(&self, project_id: &str) -> Result<(Option<String>, bool)> {
        self.c()
            .query_row("SELECT vault_path, vault_write FROM project WHERE id=?1", [project_id], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)))
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("project {project_id}")))
    }

    pub fn set_project_vault(&self, project_id: &str, path: Option<&str>, write: bool) -> Result<()> {
        let n = self.c().execute("UPDATE project SET vault_path=?2, vault_write=?3 WHERE id=?1", params![project_id, path, write as i64])?;
        if n == 0 {
            return Err(ForgeError::NotFound(format!("project {project_id}")));
        }
        Ok(())
    }
}

fn source_from(r: &Row) -> rusqlite::Result<KSource> {
    Ok(KSource { id: r.get(0)?, project_id: r.get(1)?, kind: r.get(2)?, locator: r.get(3)?, git_blob: r.get(4)?, raw_sha256: r.get(5)?, norm_sha256: r.get(6)?, snapshot: r.get(7)?, extractor: r.get(8)?, state: r.get(9)?, captured_at: r.get(10)?, checked_at: r.get(11)? })
}

fn revision_from(r: &Row) -> rusqlite::Result<KRevision> {
    Ok(KRevision { id: r.get(0)?, page_id: r.get(1)?, base_sha256: r.get(2)?, text: r.get(3)?, author: r.get(4)?, note: r.get(5)?, state: r.get(6)?, created_at: r.get(7)?, decided_at: r.get(8)? })
}

/// How a wikilink target and a page name are compared: Unicode-composed (NFC) and lower-cased.
fn link_key(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.trim().nfc().collect::<String>().to_lowercase()
}
