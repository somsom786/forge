//! Memory retrieval evaluation on real documentation: FAQ files from public projects, where the question is the query
//! and its own answer is the one right page. Pages hold the answer text only (titled "Answer N"), so the question's
//! wording is not copied into the page it should find. All pages are verified, so trust rules do not hide any.
//!
//! Measured through `retrieve::build` (what a mission receives) and, for reference, the raw full-text search under it.
//! Not part of the normal suite. Run with
//! `FORGE_MEMORY_EVAL=/path/FAQ.md,/path/faqs.md cargo test --lib memory_eval -- --nocapture`.

use super::page::Author;
use super::retrieve;
use super::service::{self, Ctx, NewPage};
use crate::git;
use crate::store::Store;

/// (question, answer) pairs from `<h3 …>question</h3>` blocks or `###` headings (an `#### Answer` sub-heading is dropped).
fn faq_pairs(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut q: Option<String> = None;
    let mut a = String::new();
    let mut lines = text.lines().peekable();
    let push = |q: &mut Option<String>, a: &mut String, out: &mut Vec<(String, String)>| {
        if let Some(qq) = q.take() {
            let body = a.trim().to_string();
            if body.split_whitespace().count() >= 3 {
                out.push((qq, body));
            }
        }
        a.clear();
    };
    while let Some(l) = lines.next() {
        if l.starts_with("<h3") {
            push(&mut q, &mut a, &mut out);
            let mut qq = String::new();
            for n in lines.by_ref() {
                if n.starts_with("</h3>") {
                    break;
                }
                qq.push_str(n.trim());
                qq.push(' ');
            }
            q = Some(qq.trim().to_string());
        } else if let Some(h) = l.strip_prefix("### ") {
            push(&mut q, &mut a, &mut out);
            q = Some(h.trim().trim_matches('`').to_string());
        } else if l.starts_with("## ") || l.starts_with("# ") {
            push(&mut q, &mut a, &mut out);
        } else if l.trim() != "#### Answer" && q.is_some() {
            a.push_str(l);
            a.push('\n');
        }
    }
    push(&mut q, &mut a, &mut out);
    out
}

#[test]
fn memory_eval_faq_retrieval() {
    let Ok(list) = std::env::var("FORGE_MEMORY_EVAL") else {
        eprintln!("SKIPPED: set FORGE_MEMORY_EVAL to FAQ markdown files");
        return;
    };
    for path in list.split(',').filter(|s| !s.is_empty()) {
        let pairs = faq_pairs(&std::fs::read_to_string(path).unwrap());
        let store = Store::in_memory();
        let repo = git::testutil::init_repo();
        let home = tempfile::tempdir().unwrap();
        let project = store.add_project("eval", &repo.path().to_string_lossy(), "main").unwrap().id;
        let ctx = Ctx { store: &store, home: home.path() };
        let ids: Vec<String> = pairs
            .iter()
            .enumerate()
            .map(|(i, (_, a))| {
                let np = NewPage { title: format!("Answer {i}"), kind: "note".into(), body: a.clone(), files: vec![], tags: vec![], topic: None, author: Author::User, sources: vec![] };
                let p = service::create_page(ctx, &project, repo.path(), np).unwrap();
                store.k_verify(&p.id, &p.current_sha256, Some("eval")).unwrap().id
            })
            .collect();
        let (mut hit1, mut hit3, mut included, mut mrr, mut fts1, mut chars) = (0.0, 0.0, 0.0, 0.0, 0.0, 0usize);
        let mut misses = vec![];
        for (i, (q, _)) in pairs.iter().enumerate() {
            let pkg = retrieve::build(ctx, &project, repo.path(), q, &[], &[], retrieve::DEFAULT_BUDGET_CHARS).unwrap();
            chars += pkg.text.chars().count();
            let pos = pkg.items.iter().position(|it| it.page_id == ids[i]);
            if let Some(p) = pos {
                included += 1.0;
                mrr += 1.0 / (p + 1) as f64;
                hit1 += if p == 0 { 1.0 } else { 0.0 };
                hit3 += if p < 3 { 1.0 } else { 0.0 };
            } else {
                misses.push(q.clone());
            }
            let fts_q = super::query::fts_match(q);
            let raw = fts_q.as_deref().map(|f| store.k_search(&project, f, 10).unwrap()).unwrap_or_default();
            fts1 += if raw.first().is_some_and(|(p, _)| p.id == ids[i]) { 1.0 } else { 0.0 };
        }
        let n = pairs.len() as f64;
        println!("\n== {path}: {} question/answer pairs", pairs.len());
        println!("retrieve::build  hit@1 {:.2}  hit@3 {:.2}  MRR {:.2}  answer included at all {:.2}  mean package {} chars", hit1 / n, hit3 / n, mrr / n, included / n, chars / pairs.len().max(1));
        println!("raw FTS (BM25)   hit@1 {:.2}", fts1 / n);
        for m in misses.iter().take(8) {
            println!("  missed: {m}");
        }
        assert!(pairs.len() >= 5, "{path}: parsed only {} pairs", pairs.len());
    }
}
