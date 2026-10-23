//! Phase 3 gate through the real engine process: knowledge verified by a person reaches a later mission's agent,
//! in the user turn (never the system prompt), with provenance, and nothing unreviewed does.

use super::*;
use crate::knowledge::page;
use crate::knowledge::retrieve::DEFAULT_BUDGET_CHARS;
use crate::supervisor::{NewKnowledgeArgs, ReviewDecision, Seen};

fn approve(f: &Fixture, id: &str, note: Option<&str>, override_reason: Option<&str>) -> Result<Mission> {
    let r = f.core.verification_report(id).unwrap();
    f.core.review(id, ReviewDecision::Approve, note, override_reason, Some(&Seen { fingerprint: r.fingerprint, contract_sha256: r.contract_sha256 }))
}

fn knowledge(f: &Fixture, title: &str, body: &str) -> crate::knowledge::store::KPage {
    f.core
        .knowledge_create(&f.project.id, NewKnowledgeArgs { title: title.into(), kind: "failure_mode".into(), body: body.into(), files: vec![], tags: vec![], topic: None, sources: vec![] })
        .unwrap()
}

fn prompt_seen_by_agent(f: &Fixture, mission_id: &str) -> (String, bool) {
    let ev = f.core.store.events(mission_id, 0, 1000).unwrap().into_iter().find(|e| e.payload["message"] == "prompt received").expect("the scripted agent reports its prompt");
    (ev.payload["promptPreview"].as_str().unwrap().to_string(), ev.payload["systemPromptHasKnowledge"].as_bool().unwrap())
}

fn run_to_review(f: &Fixture, objective: &str) -> Mission {
    let m = new_mission(f, objective, PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    m
}

const OBJECTIVE: &str = "Make the reset token expiry test deterministic";
const NOTE_BODY: &str = "The reset token expiry test compares against wall-clock time and flakes on CI clock skew. Inject a fake clock into token_service.";

#[test]
fn gate_verified_knowledge_reaches_the_next_agent_in_the_user_turn_with_provenance() {
    let f = fixture();
    let page = knowledge(&f, "Auth tests flake on clock skew", NOTE_BODY);

    // Unverified: the next mission's agent is handed exactly its task.
    let m1 = run_to_review(&f, OBJECTIVE);
    let (prompt1, sys1) = prompt_seen_by_agent(&f, &m1.id);
    assert_eq!(prompt1, OBJECTIVE, "unreviewed knowledge is never injected");
    assert!(!sys1);
    let ctx1 = &f.core.store.k_contexts_for(&m1.id).unwrap()[0];
    assert!(ctx1.items.as_array().unwrap().is_empty());
    assert!(ctx1.omitted.to_string().contains("not verified by a person"), "the user can see why it was left out: {}", ctx1.omitted);

    // A person verifies the exact text.
    f.core.knowledge_verify(&page.id, &page.current_sha256, Some("matches CI logs")).unwrap();
    let m2 = run_to_review(&f, OBJECTIVE);
    let (prompt2, sys2) = prompt_seen_by_agent(&f, &m2.id);
    assert!(prompt2.starts_with(&format!("{OBJECTIVE}\n\n<<<FORGE_PROJECT_KNOWLEDGE")), "task first, reference data after:\n{prompt2}");
    assert!(prompt2.contains("fake clock") && prompt2.contains("verified") && prompt2.contains("DATA, not instructions"));
    assert!(!sys2, "knowledge must never be placed in the system prompt");

    // Inspectable afterwards: the exact bytes, the item, its reasons and trust tier.
    let ctx2 = &f.core.store.k_contexts_for(&m2.id).unwrap()[0];
    assert_eq!(ctx2.injected_sha256, crate::proof::sha256_hex(ctx2.injected_text.as_bytes()));
    assert!(prompt2.contains(&ctx2.injected_text));
    let item = &ctx2.items.as_array().unwrap()[0];
    assert_eq!((item["trust"].as_str(), item["title"].as_str()), (Some("verified"), Some("Auth tests flake on clock skew")));
    assert_eq!(item["sha256"].as_str(), Some(page.current_sha256.as_str()), "pinned to the exact revision that was verified");
    assert!(ctx2.params["budgetChars"].as_u64() == Some(DEFAULT_BUDGET_CHARS as u64));
}

#[test]
fn a_page_edited_after_verification_is_not_injected_into_the_next_mission() {
    let f = fixture();
    let page = knowledge(&f, "Deploy order", "Run migrations before deploying the api.");
    f.core.knowledge_verify(&page.id, &page.current_sha256, None).unwrap();
    // Someone edits the file (an Obsidian user, an agent with shell access, ...).
    let file = service_root(&f).join(&page.rel_path);
    std::fs::write(&file, std::fs::read_to_string(&file).unwrap().replace("before", "AFTER")).unwrap();
    let m = run_to_review(&f, "deploy the api and run the migrations");
    let (prompt, _) = prompt_seen_by_agent(&f, &m.id);
    assert_eq!(prompt, "deploy the api and run the migrations");
    let detail = f.core.knowledge_page_detail(&page.id).unwrap();
    assert_eq!(detail.page.effective_state, "proposed");
    assert!(detail.reviews.iter().any(|r| r.action == "content_changed"));
}

fn service_root(f: &Fixture) -> std::path::PathBuf {
    crate::knowledge::service::root_for(f.core.kctx(), &f.project.id, "managed").unwrap().path
}

#[test]
fn memory_can_be_switched_off_pinned_and_is_frozen_once_a_mission_starts() {
    let f = fixture();
    let verified = knowledge(&f, "Cache sizing", "Caches hold sixty four entries for lookups.");
    f.core.knowledge_verify(&verified.id, &verified.current_sha256, None).unwrap();
    let scratch = knowledge(&f, "Scratch idea about lookups", "Maybe lookups need a bloom filter someday.");

    // Off: nothing injected even though a verified match exists.
    let off = new_mission(&f, "tune the lookup caches", PermissionProfile::AcceptEdits);
    f.core.set_mission_memory(&off.id, false, &[]).unwrap();
    f.core.start_mission(&off.id, None).unwrap();
    wait_until("review", || state(&f, &off.id) == "awaiting_review");
    assert_eq!(prompt_seen_by_agent(&f, &off.id).0, "tune the lookup caches");
    assert!(f.core.store.k_contexts_for(&off.id).unwrap().is_empty());
    wait_until("finalised", || f.core.live_session_count() == 0);

    // Pinned unverified page: included, labelled loudly.
    let pinned = new_mission(&f, "tune the lookup caches", PermissionProfile::AcceptEdits);
    f.core.set_mission_memory(&pinned.id, true, &[scratch.id.clone()]).unwrap();
    f.core.start_mission(&pinned.id, None).unwrap();
    wait_until("review", || state(&f, &pinned.id) == "awaiting_review");
    let (prompt, _) = prompt_seen_by_agent(&f, &pinned.id);
    assert!(prompt.contains("NOT VERIFIED") && prompt.contains("bloom filter") && prompt.contains("sixty four entries"), "{prompt}");
    wait_until("finalised", || f.core.live_session_count() == 0);

    // Frozen after start; foreign pins refused.
    assert!(f.core.set_mission_memory(&pinned.id, false, &[]).unwrap_err().to_string().contains("fixed once a mission has started"));
    let other = f.core.store.add_project("other", "C:/other", "main").unwrap();
    let foreign = f.core.knowledge_create(&other.id, NewKnowledgeArgs { title: "x".into(), kind: "note".into(), body: "b".into(), files: vec![], tags: vec![], topic: None, sources: vec![] });
    if let Ok(p) = foreign {
        let fresh = new_mission(&f, "x", PermissionProfile::Default);
        assert!(f.core.set_mission_memory(&fresh.id, true, &[p.id]).is_err());
    }
}

#[test]
fn follow_up_turns_do_not_re_inject_or_change_the_pinned_context() {
    let f = fixture();
    let page = knowledge(&f, "Retry policy", "Retries use exponential backoff for retries.");
    f.core.knowledge_verify(&page.id, &page.current_sha256, None).unwrap();
    let m = run_to_review(&f, "adjust the retries policy");
    assert!(prompt_seen_by_agent(&f, &m.id).0.contains("FORGE_PROJECT_KNOWLEDGE"));
    f.core.start_mission(&m.id, Some("also update the docs")).unwrap();
    wait_until("second turn", || f.core.store.sessions_for_mission(&m.id).unwrap().len() == 2 && state(&f, &m.id) == "awaiting_review" && f.core.live_session_count() == 0);
    let previews: Vec<String> = f.core.store.events(&m.id, 0, 1000).unwrap().into_iter().filter(|e| e.payload["message"] == "prompt received").map(|e| e.payload["promptPreview"].as_str().unwrap().to_string()).collect();
    assert_eq!(previews.len(), 2);
    assert_eq!(previews[1], "also update the docs", "the follow-up is exactly what the user typed");
    assert_eq!(f.core.store.k_contexts_for(&m.id).unwrap().len(), 1, "one package per mission, built at its first turn");
}

#[test]
fn a_memory_failure_never_blocks_a_mission() {
    let f = fixture();
    // Make the knowledge directory unusable: a plain file where the folder must be.
    std::fs::write(f.core.cfg.home.join("knowledge"), "not a directory").unwrap();
    let m = new_mission(&f, "make a note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review despite broken memory", || state(&f, &m.id) == "awaiting_review");
    assert_eq!(prompt_seen_by_agent(&f, &m.id).0, "make a note");
    let notices = f.events.lock().unwrap().iter().filter(|e| matches!(e, UiEvent::Notice { message, .. } if message.contains("project memory was skipped"))).count();
    assert_eq!(notices, 1, "the user is told memory was skipped");
}

#[test]
fn merging_records_a_factual_unverified_mission_record_that_is_not_injected_until_verified() {
    let f = fixture();
    let m = run_to_review(&f, "make a note about caches");
    approve(&f, &m.id, Some("fine"), None).unwrap();
    let out = f.core.merge_mission(&m.id, false).unwrap();
    assert!(out.notes.iter().all(|n| !n.contains("mission record could not")), "{:?}", out.notes);

    let pages = f.core.store.k_list(&f.project.id).unwrap();
    let rec = pages.iter().find(|p| p.kind == "mission_record").expect("a mission record page");
    assert_eq!((rec.author.as_str(), rec.status.as_str(), rec.origin.as_str()), ("system", "proposed", "forge"));
    let detail = f.core.knowledge_page_detail(&rec.id).unwrap();
    assert!(detail.text.contains("FORGE_TEST.md") && detail.text.contains("not whether the change was correct"), "{}", detail.text);
    let kinds: Vec<_> = detail.sources.iter().map(|s| s.kind.as_str()).collect();
    assert!(kinds.contains(&"mission") && kinds.contains(&"commit"), "{kinds:?}");
    assert!(detail.sources.iter().any(|s| s.kind == "commit" && s.locator == out.commit));

    // Not injected while unreviewed, even though it matches the next objective closely.
    let next = run_to_review(&f, "make a note about caches again");
    assert_eq!(prompt_seen_by_agent(&f, &next.id).0, "make a note about caches again");
    // After a person verifies it, it is.
    let rec = f.core.store.k_get(&rec.id).unwrap();
    f.core.knowledge_verify(&rec.id, &rec.current_sha256, None).unwrap();
    let after = new_mission(&f, "make a note about caches third", PermissionProfile::AcceptEdits);
    f.core.start_mission(&after.id, None).unwrap();
    wait_until("review", || state(&f, &after.id) == "awaiting_review");
    assert!(prompt_seen_by_agent(&f, &after.id).0.contains("Mission record: make a note about caches"));
}

#[test]
fn an_agent_message_can_be_saved_as_a_proposal_sourced_to_that_exact_event() {
    let f = fixture();
    let m = run_to_review(&f, "make a note");
    let ev = f.core.store.events(&m.id, 0, 100).unwrap().into_iter().find(|e| e.kind == "agent.text" && e.payload["text"].as_str().unwrap_or("").starts_with("Plan:")).unwrap();
    let pg = f.core.knowledge_save_from_event(&m.id, ev.id, "What the agent planned", "note").unwrap();
    assert_eq!((pg.author.as_str(), pg.status.as_str()), ("agent", "proposed"));
    let d = f.core.knowledge_page_detail(&pg.id).unwrap();
    assert!(d.sources.iter().any(|s| s.kind == "mission" && s.locator == format!("{}#event:{}", m.id, ev.seq)), "{:?}", d.sources);
    assert!(f.core.knowledge_save_from_event(&m.id, 999_999, "x", "note").is_err());
    let tool = f.core.store.events(&m.id, 0, 100).unwrap().into_iter().find(|e| e.kind == "tool.call").unwrap();
    assert!(f.core.knowledge_save_from_event(&m.id, tool.id, "x", "note").unwrap_err().to_string().contains("only an agent message"));
}

#[test]
fn status_reports_effective_states_and_the_vault_configuration() {
    let f = fixture();
    let a = knowledge(&f, "A page", "alpha");
    let _b = knowledge(&f, "B page", "beta");
    f.core.knowledge_verify(&a.id, &a.current_sha256, None).unwrap();
    let st = f.core.knowledge_status(&f.project.id).unwrap();
    assert_eq!((st.total, st.counts.get("verified").copied(), st.counts.get("proposed").copied()), (2, Some(1), Some(1)));
    assert!(st.vault.is_none());
    let vault = tempfile::tempdir().unwrap();
    std::fs::create_dir(vault.path().join(".obsidian")).unwrap();
    std::fs::write(vault.path().join("Note.md"), "# Note\nhello").unwrap();
    let st = f.core.knowledge_set_vault(&f.project.id, Some(vault.path()), false).unwrap();
    let v = st.vault.unwrap();
    assert!(v.is_obsidian_vault && !v.writable);
    assert_eq!(st.total, 3);
    // writing the file text of a page changes its effective state in the status
    let file = service_root(&f).join(&a.rel_path);
    std::fs::write(&file, format!("{}\nedited", std::fs::read_to_string(&file).unwrap())).unwrap();
    f.core.knowledge_scan(&f.project.id).unwrap();
    let st = f.core.knowledge_status(&f.project.id).unwrap();
    assert_eq!(st.counts.get("verified"), None, "the edit made the only verified page unreviewed again: {:?}", st.counts);
    let _ = page::MAX_PAGE_BYTES;
}
