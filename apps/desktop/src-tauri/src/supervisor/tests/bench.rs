//! Stress and performance measurements at volumes beyond normal use. Not part of the normal suite (they take a while and
//! their numbers depend on the machine): run with `FORGE_BENCH=1 cargo test --release --lib bench -- --nocapture`.
//! Each one also asserts correctness at that volume, which is the part that must never regress.

use super::*;
use std::time::Instant;

fn on() -> bool {
    let yes = std::env::var("FORGE_BENCH").as_deref() == Ok("1");
    if !yes {
        eprintln!("SKIPPED: set FORGE_BENCH=1");
    }
    yes
}

/// Like `fixture()`, but with the on-disk SQLite database (WAL, synchronous=NORMAL) the app uses, so the numbers mean
/// something.
fn disk_fixture() -> Fixture {
    let repo = git::testutil::init_repo();
    let home = tempfile::tempdir().unwrap();
    let events: Arc<StdMutex<Vec<UiEvent>>> = Arc::new(StdMutex::new(vec![]));
    let ev2 = events.clone();
    let core = Core::new(
        Store::open(&home.path().join("forge.db")).unwrap(),
        Config { home: home.path().to_path_buf(), node: None, engine_script: Some(engine_script()), claude: None, codex: None, allow_fake: true, max_concurrent: 2, ollama_url: "http://127.0.0.1:11434".into(), engine_env: vec![] },
        Arc::new(move |e| ev2.lock().unwrap().push(e)),
    );
    let project = core.add_project(repo.path()).unwrap();
    Fixture { core, repo, _home: home, events, project }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn db_bytes(f: &Fixture) -> u64 {
    ["forge.db", "forge.db-wal"].iter().map(|n| std::fs::metadata(f.core.cfg.home.join(n)).map(|m| m.len()).unwrap_or(0)).sum()
}

#[test]
fn bench_event_ingestion_and_reading_a_long_transcript() {
    if !on() {
        return;
    }
    let f = disk_fixture();
    let m = new_mission(&f, "bench", PermissionProfile::Default);
    let s = f.core.store.create_session(&m.id, "fake", None).unwrap();
    const N: i64 = 200_000;
    let payload = json!({"text": "x".repeat(200), "tool": "Edit", "path": "src/lib/some/file.rs"});
    let t = Instant::now();
    for i in 0..N {
        f.core.store.insert_event(&m.id, &s.id, i, "agent.text", &payload).unwrap();
    }
    let ingest = ms(t);
    // Re-delivery of the same events is ignored, not duplicated.
    for i in 0..1000 {
        assert!(f.core.store.insert_event(&m.id, &s.id, i, "agent.text", &payload).unwrap().is_none());
    }
    let t = Instant::now();
    let first = f.core.store.events(&m.id, 0, 500).unwrap();
    let first_ms = ms(t);
    let last_id = first.last().unwrap().id;
    let t = Instant::now();
    let mut after = 0;
    let mut pages = 0;
    loop {
        let page = f.core.store.events(&m.id, after, 1000).unwrap();
        if page.is_empty() {
            break;
        }
        after = page.last().unwrap().id;
        pages += 1;
    }
    let all_ms = ms(t);
    println!(
        "\nevents: {N} inserted in {ingest:.0} ms ({:.0}/s, one transaction each); first page of 500 in {first_ms:.1} ms; full read in {pages} pages of 1000 in {all_ms:.0} ms; database {} MB",
        N as f64 / (ingest / 1000.0),
        db_bytes(&f) / 1_000_000
    );
    assert!(last_id > 0 && pages == (N as usize).div_ceil(1000));
}

#[test]
fn bench_many_missions_listing_and_recovery() {
    if !on() {
        return;
    }
    let f = disk_fixture();
    const N: usize = 5_000;
    let t = Instant::now();
    for i in 0..N {
        new_mission(&f, &format!("mission {i}"), PermissionProfile::Default);
    }
    let create = ms(t);
    // A tenth are "running" when FORGE restarts.
    f.core.store.c().execute("UPDATE mission SET state='running' WHERE rowid % 10 = 0", []).unwrap();
    let t = Instant::now();
    let listed = f.core.store.list_missions(None).unwrap();
    let list_ms = ms(t);
    let t = Instant::now();
    let report = f.core.recover().unwrap();
    let recover_ms = ms(t);
    let interrupted = f.core.store.missions_in_states(&[crate::mission::MissionState::Interrupted]).unwrap().len();
    println!("\nmissions: {N} created in {create:.0} ms; list (capped at {} rows) in {list_ms:.1} ms; recovery of {interrupted} interrupted in {recover_ms:.0} ms", listed.len());
    assert_eq!(interrupted, N / 10, "recovery reaches every active mission, not only a listed page: {report:?}");
}

#[test]
fn bench_code_index_on_a_twenty_thousand_file_repository() {
    if !on() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    git::run(root, &["init", "-q", "-b", "main"]).unwrap();
    const FILES: usize = 20_000;
    for i in 0..FILES {
        let dir = root.join(format!("pkg{}/mod{}", i / 500, (i / 50) % 10));
        std::fs::create_dir_all(&dir).unwrap();
        let ext = ["rs", "ts", "py", "js"][i % 4];
        let body = match ext {
            "rs" => format!("pub struct Thing{i} {{ v: u32 }}\nimpl Thing{i} {{\n    pub fn handle_request_{i}(&self) -> u32 {{ self.v }}\n}}\npub fn parse_config_{i}() {{}}\n"),
            "ts" => format!("export class Service{i} {{\n  fetchOrders{i}(): number {{ return {i}; }}\n}}\nexport const RETRY_{i} = 3;\n"),
            "py" => format!("class Model{i}:\n    def save_{i}(self):\n        return {i}\n\ndef load_{i}():\n    pass\n"),
            _ => format!("function render{i}() {{ return {i}; }}\nmodule.exports.helper{i} = function helper{i}() {{}};\n"),
        };
        std::fs::write(dir.join(format!("f{i}.{ext}")), body).unwrap();
    }
    git::run(root, &["add", "-A"]).unwrap();
    git::run(root, &["-c", "user.email=b@x", "-c", "user.name=b", "-c", "commit.gpgsign=false", "commit", "-qm", "bench"]).unwrap();
    let t = Instant::now();
    let ix = crate::codeintel::build(root).unwrap();
    let build_ms = ms(t);
    let t = Instant::now();
    let sel = crate::codeintel::select_files(&ix, "fix the retry in fetchOrders1233 for the order service", &[], 5);
    let q_ms = ms(t);
    println!("\ncode index: {} files, {} symbols in {build_ms:.0} ms (truncated: {:?}); a query in {q_ms:.1} ms -> {:?}", ix.files.len(), ix.symbols.len(), ix.truncated, &sel[..sel.len().min(2)]);
    assert_eq!(ix.files.len(), FILES);
    assert_eq!(sel.first().map(String::as_str), Some("pkg2/mod4/f1233.ts"), "the file defining the named method (file 1233 is TypeScript: i % 4 == 1) ranks first at this size");
}

#[test]
fn bench_memory_with_three_thousand_notes() {
    if !on() {
        return;
    }
    let f = disk_fixture();
    let vault = tempfile::tempdir().unwrap();
    const N: usize = 3_000;
    for i in 0..N {
        let topic = ["caching", "auth", "billing", "search", "deploy"][i % 5];
        std::fs::write(vault.path().join(format!("note-{i}.md")), format!("# Note {i}\n\nAbout {topic}: decision {i} links [[note-{}]].\n{}\n", (i + 1) % N, "Background text. ".repeat(40))).unwrap();
    }
    let ctx = crate::knowledge::service::Ctx { store: &f.core.store, home: &f.core.cfg.home };
    crate::knowledge::service::set_vault(ctx, &f.project.id, Some(vault.path()), false).unwrap();
    let t = Instant::now();
    let first = crate::knowledge::service::scan(ctx, &f.project.id).unwrap();
    let scan_ms = ms(t);
    let t = Instant::now();
    let second = crate::knowledge::service::scan(ctx, &f.project.id).unwrap();
    let rescan_ms = ms(t);
    let t = Instant::now();
    let pkg = crate::knowledge::retrieve::build(ctx, &f.project.id, f.repo.path(), "how does billing decision 1234 work", &[], &[], crate::knowledge::retrieve::DEFAULT_BUDGET_CHARS).unwrap();
    let build_ms = ms(t);
    println!("\nmemory: {N} notes scanned in {scan_ms:.0} ms; unchanged rescan in {rescan_ms:.0} ms; a mission's context package in {build_ms:.0} ms ({} items; unverified notes are never injected)", pkg.items.len());
    assert_eq!(first.indexed, N);
    assert_eq!(second.unchanged, N, "an unchanged vault is not re-read");
}

#[test]
fn bench_storage_cleanup_of_many_old_missions() {
    if !on() {
        return;
    }
    let f = disk_fixture();
    const MISSIONS: usize = 200;
    const EVENTS: i64 = 500;
    let payload = json!({"text": "y".repeat(300)});
    for i in 0..MISSIONS {
        let m = new_mission(&f, &format!("old {i}"), PermissionProfile::Default);
        let s = f.core.store.create_session(&m.id, "fake", None).unwrap();
        for e in 0..EVENTS {
            f.core.store.insert_event(&m.id, &s.id, e, "agent.text", &payload).unwrap();
        }
        f.core.store.c().execute("UPDATE mission SET state='cancelled', updated_at='2020-01-01T00:00:00.000Z' WHERE id=?1", [&m.id]).unwrap();
    }
    let before = db_bytes(&f);
    let t = Instant::now();
    let plan = f.core.storage_cleanup_preview(30).unwrap();
    let preview_ms = ms(t);
    let t = Instant::now();
    let res = f.core.storage_cleanup_apply(30, &plan.token).unwrap();
    let apply_ms = ms(t);
    println!("\nclean-up: {} missions / {} events previewed in {preview_ms:.0} ms, removed in {apply_ms:.0} ms; database {} MB before (space is reused, the file does not shrink without VACUUM)", plan.missions.len(), plan.events, before / 1_000_000);
    assert_eq!(res.events_deleted, MISSIONS as i64 * EVENTS);
}
