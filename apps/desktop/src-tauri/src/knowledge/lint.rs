//! Memory quality checks. Advisory only: a lint finding never changes a page's state by itself.
//!
//! Implemented: broken/ambiguous wikilinks, references to files that no longer exist, orphans, stale sources, missing
//! files, near-duplicate pages, long-unreviewed proposals, edits since verification, parse warnings, conflicting
//! pages on one topic. NOT implemented: semantic contradiction detection (that needs a model); the topic-conflict check
//! is only "two currently-verified pages claim the same topic key", which is a prompt for a human, not a verdict.

use super::service::Ctx;
use super::store::KPage;
use crate::error::Result;
use crate::git;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// info | warning | problem
    pub severity: String,
    pub kind: String,
    pub page_id: Option<String>,
    pub message: String,
}

fn issue(sev: &str, kind: &str, page: Option<&KPage>, msg: impl Into<String>) -> Issue {
    Issue { severity: sev.into(), kind: kind.into(), page_id: page.map(|p| p.id.clone()), message: msg.into() }
}

fn shingles(text: &str) -> HashSet<String> {
    let words: Vec<String> = text.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 1).map(|w| w.to_lowercase()).collect();
    if words.len() < 3 {
        return words.into_iter().collect();
    }
    words.windows(3).map(|w| w.join(" ")).collect()
}

pub fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    a.intersection(b).count() as f64 / a.union(b).count() as f64
}

pub fn run(ctx: Ctx, project_id: &str, repo_root: &Path) -> Result<Vec<Issue>> {
    let pages: Vec<KPage> = ctx.store.k_list(project_id)?;
    let mut out = vec![];
    let live: Vec<&KPage> = pages.iter().filter(|p| !p.missing).collect();
    let now = chrono::Utc::now();

    for p in &pages {
        if p.missing {
            out.push(issue("problem", "missing_file", Some(p), format!("\"{}\": the page file no longer exists ({})", p.title, p.rel_path)));
            continue;
        }
        for w in &p.warnings {
            out.push(issue("warning", "parse_warning", Some(p), format!("\"{}\": {w}", p.title)));
        }
        if p.modified_since_verified {
            out.push(issue("info", "edited_since_verified", Some(p), format!("\"{}\" was edited after it was verified, so it is unreviewed again", p.title)));
        }
        if p.status == "proposed" {
            let age = chrono::DateTime::parse_from_rfc3339(&p.created_at).map(|t| (now - t.with_timezone(&chrono::Utc)).num_days()).unwrap_or(0);
            if age > 30 {
                out.push(issue("info", "unreviewed_for_long", Some(p), format!("\"{}\" has been waiting for review for {age} days: verify, dispute or archive it", p.title)));
            }
        }
        // links
        let links = ctx.store.k_links(&p.id)?;
        for l in &links {
            match (l.kind.as_str(), l.ambiguous, &l.to_page) {
                ("wikilink", true, _) => out.push(issue("warning", "ambiguous_link", Some(p), format!("\"{}\": [[{}]] matches more than one page; link by a unique title", p.title, l.target))),
                ("wikilink", false, None) => out.push(issue("warning", "broken_link", Some(p), format!("\"{}\": [[{}]] points at no page", p.title, l.target))),
                ("file", _, _) => {
                    let exists = git::run(repo_root, &["rev-parse", "--verify", "--quiet", &format!("HEAD:{}", l.target)]).is_ok();
                    if !exists {
                        out.push(issue("warning", "broken_file_ref", Some(p), format!("\"{}\": refers to {} which is not in the repository at HEAD", p.title, l.target)));
                    }
                }
                _ => {}
            }
        }
        // orphan: nothing links to it, it links to nothing, names no files, and is not a generated record
        let inbound = ctx.store.k_backlinks(&p.id)?.len();
        if inbound == 0 && links.is_empty() && p.kind != "mission_record" && p.files.is_empty() {
            out.push(issue("info", "orphan", Some(p), format!("\"{}\" is not linked from anywhere and names no files; it may never be retrieved", p.title)));
        }
        for s in ctx.store.k_sources_for(&p.id)? {
            match s.state.as_str() {
                "changed" => out.push(issue("warning", "stale_source", Some(p), format!("\"{}\": its source {} changed after the page was written", p.title, s.locator))),
                "missing" => out.push(issue("warning", "missing_source", Some(p), format!("\"{}\": its source {} no longer exists", p.title, s.locator))),
                _ => {}
            }
        }
        if p.status == "superseded" && p.superseded_by_id.as_deref().is_none_or(|id| ctx.store.k_get(id).is_err()) {
            out.push(issue("problem", "dangling_supersession", Some(p), format!("\"{}\" is superseded by a page that no longer exists", p.title)));
        }
    }

    // near duplicates (advisory)
    let sh: Vec<(&KPage, HashSet<String>)> = live.iter().filter(|p| p.status != "archived").map(|p| (*p, shingles(&ctx.store.k_body(&p.id).unwrap_or_default()))).collect();
    for i in 0..sh.len() {
        for j in (i + 1)..sh.len() {
            let sim = jaccard(&sh[i].1, &sh[j].1);
            if sim >= 0.8 {
                out.push(issue("warning", "near_duplicate", Some(sh[i].0), format!("\"{}\" and \"{}\" are very similar ({:.0}% shared phrasing); consider merging or superseding", sh[i].0.title, sh[j].0.title, sim * 100.0)));
            }
        }
    }

    // two currently-verified pages claiming one topic
    let mut by_topic: std::collections::BTreeMap<&str, Vec<&KPage>> = Default::default();
    for p in live.iter().filter(|p| p.effective_state == "verified") {
        if let Some(t) = p.topic.as_deref().filter(|t| !t.is_empty()) {
            by_topic.entry(t).or_default().push(p);
        }
    }
    for (topic, ps) in by_topic {
        if ps.len() > 1 {
            out.push(issue("warning", "topic_conflict", Some(ps[0]), format!("{} verified pages share the topic \"{topic}\" ({}); a person should decide which is current and supersede the others", ps.len(), ps.iter().map(|p| format!("\"{}\"", p.title)).collect::<Vec<_>>().join(", "))));
        }
    }

    // index health
    let expected = live.len() as i64;
    let actual = ctx.store.k_fts_count(project_id)?;
    if actual != expected {
        out.push(issue("problem", "index_drift", None, format!("the search index has {actual} entries but there are {expected} live pages; run a rebuild")));
    }
    out.sort_by_key(|i| match i.severity.as_str() {
        "problem" => 0,
        "warning" => 1,
        _ => 2,
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::page::Author;
    use crate::knowledge::service::{self, NewPage, SourceSpec};
    use crate::store::Store;

    fn env() -> (Store, tempfile::TempDir, tempfile::TempDir, String) {
        let store = Store::in_memory();
        let repo = git::testutil::init_repo();
        let home = tempfile::tempdir().unwrap();
        let p = store.add_project("demo", &repo.path().to_string_lossy(), "main").unwrap();
        (store, home, repo, p.id)
    }

    fn page(s: &Store, home: &Path, repo: &Path, pid: &str, title: &str, body: &str, files: &[&str], topic: Option<&str>, sources: Vec<SourceSpec>) -> KPage {
        service::create_page(Ctx { store: s, home }, pid, repo, NewPage { title: title.into(), kind: "decision".into(), body: body.into(), files: files.iter().map(|f| f.to_string()).collect(), tags: vec![], topic: topic.map(Into::into), author: Author::User, sources }).unwrap()
    }

    fn kinds(i: &[Issue]) -> Vec<&str> {
        i.iter().map(|x| x.kind.as_str()).collect()
    }

    #[test]
    fn finds_broken_links_files_orphans_and_duplicates() {
        let (s, h, r, pid) = env();
        let ctx = Ctx { store: &s, home: h.path() };
        page(&s, h.path(), r.path(), &pid, "Hub", "See [[Ghost page]] and [[Spoke]].", &["src/lib.txt"], None, vec![]);
        page(&s, h.path(), r.path(), &pid, "Spoke", "Detail page about the spoke component with some words repeated here.", &["src/gone.rs"], None, vec![]);
        page(&s, h.path(), r.path(), &pid, "Lonely", "Nobody links here and it names no files at all.", &[], None, vec![]);
        let dup = "The retry policy uses exponential backoff with jitter and a hard cap of five attempts per request.";
        page(&s, h.path(), r.path(), &pid, "Retry A", dup, &["src/lib.txt"], None, vec![]);
        page(&s, h.path(), r.path(), &pid, "Retry B", dup, &["src/lib.txt"], None, vec![]);
        s.k_resolve_links(&pid).unwrap();
        let issues = run(ctx, &pid, r.path()).unwrap();
        let k = kinds(&issues);
        for want in ["broken_link", "broken_file_ref", "orphan", "near_duplicate"] {
            assert!(k.contains(&want), "missing {want} in {k:?}");
        }
        assert!(issues.iter().any(|i| i.kind == "broken_file_ref" && i.message.contains("src/gone.rs")));
        assert!(!issues.iter().any(|i| i.kind == "orphan" && i.message.contains("Hub")), "a page with outbound links is not an orphan");
        assert!(!issues.iter().any(|i| i.kind == "broken_file_ref" && i.message.contains("src/lib.txt")), "existing files are fine");
    }

    #[test]
    fn flags_stale_sources_edits_since_verification_and_topic_conflicts() {
        let (s, h, r, pid) = env();
        let ctx = Ctx { store: &s, home: h.path() };
        let a = page(&s, h.path(), r.path(), &pid, "Auth model A", "Sessions are stored server side in the database always.", &["src/lib.txt"], Some("auth"), vec![SourceSpec::RepoFile { rel: "src/lib.txt".into() }]);
        let b = page(&s, h.path(), r.path(), &pid, "Auth model B", "Sessions are JWTs signed with a rotating key and never stored.", &["src/lib.txt"], Some("auth"), vec![]);
        s.k_verify(&a.id, &a.current_sha256, None).unwrap();
        s.k_verify(&b.id, &b.current_sha256, None).unwrap();
        std::fs::write(r.path().join("src/lib.txt"), "changed\n").unwrap();
        git::run(r.path(), &["add", "-A"]).unwrap();
        git::run(r.path(), &["commit", "-q", "-m", "x"]).unwrap();
        service::refresh_sources(ctx, &pid, r.path()).unwrap();
        let f = service::root_for(ctx, &pid, "managed").unwrap().path.join(&b.rel_path);
        std::fs::write(&f, std::fs::read_to_string(&f).unwrap().replace("rotating", "static")).unwrap();
        service::scan(ctx, &pid).unwrap();
        let k = kinds(&run(ctx, &pid, r.path()).unwrap()).into_iter().map(String::from).collect::<Vec<_>>();
        assert!(k.contains(&"stale_source".to_string()), "{k:?}");
        assert!(k.contains(&"edited_since_verified".to_string()), "{k:?}");
        assert!(!k.contains(&"topic_conflict".to_string()), "B lost its verification when edited, so only A is verified on that topic: {k:?}");
        s.k_verify(&b.id, &s.k_get(&b.id).unwrap().current_sha256, None).unwrap();
        assert!(kinds(&run(ctx, &pid, r.path()).unwrap()).contains(&"topic_conflict"));
    }

    #[test]
    fn detects_index_drift_and_reports_missing_files_as_problems_first() {
        let (s, h, r, pid) = env();
        let ctx = Ctx { store: &s, home: h.path() };
        let p = page(&s, h.path(), r.path(), &pid, "Solo", "alone alone alone words words", &["src/lib.txt"], None, vec![]);
        std::fs::remove_file(service::root_for(ctx, &pid, "managed").unwrap().path.join(&p.rel_path)).unwrap();
        service::scan(ctx, &pid).unwrap();
        let issues = run(ctx, &pid, r.path()).unwrap();
        assert_eq!(issues[0].kind, "missing_file", "problems sort first: {issues:?}");
        s.c().execute("DELETE FROM knowledge_fts", []).unwrap();
        page(&s, h.path(), r.path(), &pid, "Second", "another page body with words", &["src/lib.txt"], None, vec![]);
        s.c().execute("DELETE FROM knowledge_fts", []).unwrap();
        assert!(kinds(&run(ctx, &pid, r.path()).unwrap()).contains(&"index_drift"));
        s.k_fts_rebuild(&pid).unwrap();
        assert!(!kinds(&run(ctx, &pid, r.path()).unwrap()).contains(&"index_drift"));
    }

    #[test]
    fn jaccard_basics() {
        let a: HashSet<String> = ["a b c", "b c d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(jaccard(&a, &a), 1.0);
        assert_eq!(jaccard(&a, &HashSet::new()), 0.0);
    }
}
