//! Phase 6: experiments against real temporary git repositories. Commands are `node` scripts so they behave the same on Windows
//! and Unix.

use super::*;
use crate::experiment::{Direction, Metric, NewExperiment, VariantSpec, Verdict};

fn commit_all(repo: &Path, msg: &str) {
    git::run(repo, &["add", "-A"]).unwrap();
    git::run(repo, &["commit", "-q", "-m", msg]).unwrap();
}

/// `bench.js` prints the metric it is given; `check.js` exits with the given code.
fn write_rev(repo: &Path, score: f64, check_exit: i32) {
    std::fs::write(repo.join("bench.js"), format!("console.log('running'); console.log('FORGE_METRIC score={score}');\n")).unwrap();
    std::fs::write(repo.join("check.js"), format!("process.exit({check_exit});\n")).unwrap();
}

/// main: score 10. Branch `fast`: score 5. Branch `broken`: score 1 but its correctness check fails. HEAD stays on main.
fn repo_with_revisions(f: &Fixture) {
    let r = f.repo.path();
    write_rev(r, 10.0, 0);
    commit_all(r, "baseline bench");
    for (branch, score, exit) in [("fast", 5.0, 0), ("broken", 1.0, 1)] {
        git::run(r, &["checkout", "-q", "-b", branch]).unwrap();
        write_rev(r, score, exit);
        commit_all(r, branch);
        git::run(r, &["checkout", "-q", "main"]).unwrap();
    }
}

fn spec(variants: &[(&str, &str)], reps: u32) -> NewExperiment {
    NewExperiment {
        name: "score".into(),
        objective: Some("lower the score".into()),
        command: "node bench.js".into(),
        correctness_command: Some("node check.js".into()),
        metric: Metric::Output { name: "score".into(), direction: Direction::Lower },
        repetitions: reps,
        warmup: 1,
        timeout_secs: 60,
        baseline_ref: "main".into(),
        variants: variants.iter().map(|(l, r)| VariantSpec { label: (*l).into(), git_ref: (*r).into(), mission_id: None }).collect(),
    }
}

fn worktree_count(repo: &Path) -> usize {
    git::run(repo, &["worktree", "list", "--porcelain"]).unwrap().lines().filter(|l| l.starts_with("worktree ")).count()
}

#[test]
fn a_variant_that_wins_every_run_is_named_and_the_revisions_are_pinned_and_isolated() {
    let f = fixture();
    repo_with_revisions(&f);
    let r = f.repo.path();
    let main_head = git::head_commit(r).unwrap();
    let fast_at_create = git::run(r, &["rev-parse", "fast"]).unwrap().trim().to_string();
    let e = f.core.experiment_create(&f.project.id, spec(&[("fast", "fast")], 3)).unwrap();
    assert_eq!(e.state, "draft");

    // The branch moves after the experiment was defined: the experiment still measures the recorded commit.
    git::run(r, &["checkout", "-q", "fast"]).unwrap();
    write_rev(r, 50.0, 0);
    commit_all(r, "fast got slower");
    git::run(r, &["checkout", "-q", "main"]).unwrap();

    let d = f.core.experiment_run(&e.id).unwrap();
    assert_eq!(d.experiment.state, "completed", "{:?}", d.experiment.last_error);
    assert_eq!(d.experiment.attempt, 1);
    let fast = d.variants.iter().find(|v| v.label == "fast").unwrap();
    assert_eq!(fast.commit_sha, fast_at_create);
    assert_eq!(d.variants.iter().find(|v| v.is_baseline).unwrap().commit_sha, main_head);

    let rep = d.experiment.report.clone().unwrap();
    assert_eq!(rep.conclusion, "winner", "{rep:#?}");
    assert_eq!(rep.winner.as_deref(), Some(fast.id.as_str()));
    let fv = rep.variants.iter().find(|v| v.label == "fast").unwrap();
    assert_eq!((fv.verdict, fv.stats.as_ref().unwrap().median, fv.stats.as_ref().unwrap().n), (Verdict::Better, 5.0, 3));
    assert!((fv.change_vs_baseline.unwrap() + 0.5).abs() < 1e-9);

    // Every run is recorded: 2 correctness, 2 warm-up (not measured), 6 measured, interleaved.
    let by = |p: &str| d.results.iter().filter(|x| x.phase == p).count();
    assert_eq!((by("correctness"), by("warmup"), by("measure")), (2, 2, 6));
    assert!(d.results.iter().filter(|x| x.phase == "warmup").all(|x| x.metric_value.is_some()));
    let order: Vec<&str> = d.results.iter().filter(|x| x.phase == "measure").map(|x| x.variant_id.as_str()).collect();
    assert_ne!(order[0], order[2], "measured runs rotate which revision goes first: {order:?}");
    assert!(d.results.iter().all(|x| x.output_tail.contains("running") || x.phase == "correctness"));
    let env = d.experiment.env.clone().unwrap();
    assert!(env["os"].is_string() && env["git"].as_str().unwrap().contains("git version"));

    // Worktrees are gone; the user's checkout is untouched.
    assert_eq!(worktree_count(r), 1);
    assert_eq!(git::head_commit(r).unwrap(), main_head);
    assert!(!git::status(r).unwrap().dirty);
    assert!(f.core.store.audit_count("experiment.run").unwrap() == 1);

    // A second attempt keeps the first one's runs and produces its own.
    let d2 = f.core.experiment_run(&e.id).unwrap();
    assert_eq!(d2.experiment.attempt, 2);
    assert_eq!(d2.results.len(), 10);
    assert_eq!(f.core.store.exp_results(&e.id, 1).unwrap().len(), 10, "earlier attempts are preserved");
}

#[test]
fn fewer_than_three_runs_never_name_a_winner() {
    let f = fixture();
    repo_with_revisions(&f);
    let e = f.core.experiment_create(&f.project.id, spec(&[("fast", "fast")], 2)).unwrap();
    let d = f.core.experiment_run(&e.id).unwrap();
    let rep = d.experiment.report.unwrap();
    assert_eq!((rep.winner.clone(), rep.conclusion.as_str()), (None, "inconclusive"));
    assert!(rep.variants.iter().all(|v| v.verdict == Verdict::InsufficientRuns), "{rep:#?}");
}

#[test]
fn a_revision_failing_correctness_is_not_measured_and_cannot_win() {
    let f = fixture();
    repo_with_revisions(&f);
    let e = f.core.experiment_create(&f.project.id, spec(&[("fast", "fast"), ("broken", "broken")], 3)).unwrap();
    let d = f.core.experiment_run(&e.id).unwrap();
    let broken = d.variants.iter().find(|v| v.label == "broken").unwrap();
    assert_eq!(d.results.iter().filter(|x| x.variant_id == broken.id && x.phase != "correctness").count(), 0);
    let rep = d.experiment.report.unwrap();
    let b = rep.variants.iter().find(|v| v.label == "broken").unwrap();
    assert_eq!((b.verdict, b.correctness), (Verdict::Invalid, Some(false)));
    // The faster-looking but broken revision does not win; the correct one does.
    assert_eq!(rep.winner.as_deref(), Some(d.variants.iter().find(|v| v.label == "fast").unwrap().id.as_str()));
}

#[test]
fn a_command_that_changes_its_worktree_or_prints_no_metric_invalidates_the_result() {
    let f = fixture();
    repo_with_revisions(&f);
    let mut s = spec(&[("fast", "fast")], 3);
    s.correctness_command = None;
    s.command = "node -e \"require('fs').writeFileSync('out-'+Date.now()+Math.random()+'.txt','x'); require('./bench.js')\"".into();
    let d = f.core.experiment_run(&f.core.experiment_create(&f.project.id, s).unwrap().id).unwrap();
    let rep = d.experiment.report.unwrap();
    assert!(rep.variants.iter().all(|v| v.verdict == Verdict::Invalid && v.worktree_changed), "{rep:#?}");
    assert_eq!(rep.conclusion, "inconclusive");

    let mut s = spec(&[("fast", "fast")], 3);
    s.metric = Metric::Output { name: "absent".into(), direction: Direction::Lower };
    let d = f.core.experiment_run(&f.core.experiment_create(&f.project.id, s).unwrap().id).unwrap();
    assert!(d.results.iter().filter(|x| x.phase == "measure").all(|x| x.state == "no_metric"));
    let rep = d.experiment.report.unwrap();
    assert!(rep.variants.iter().all(|v| v.verdict == Verdict::Invalid && v.failed_runs == 3), "{rep:#?}");
}

#[test]
fn wall_clock_duration_can_be_the_metric() {
    let f = fixture();
    let r = f.repo.path();
    std::fs::write(r.join("bench.js"), "const t=Date.now(); while(Date.now()-t<700){}\n").unwrap();
    commit_all(r, "slow");
    git::run(r, &["checkout", "-q", "-b", "quick"]).unwrap();
    std::fs::write(r.join("bench.js"), "\n").unwrap();
    commit_all(r, "quick");
    git::run(r, &["checkout", "-q", "main"]).unwrap();
    let mut s = spec(&[("quick", "quick")], 3);
    s.metric = Metric::Duration;
    s.correctness_command = None;
    s.warmup = 0;
    let d = f.core.experiment_run(&f.core.experiment_create(&f.project.id, s).unwrap().id).unwrap();
    let rep = d.experiment.report.unwrap();
    let base = rep.variants.iter().find(|v| v.is_baseline).unwrap().stats.clone().unwrap();
    assert!(base.min >= 700.0, "{base:?}");
    assert_eq!(rep.conclusion, "winner", "{rep:#?}");
}

#[test]
fn unknown_revisions_and_bad_definitions_are_refused_before_anything_runs() {
    let f = fixture();
    repo_with_revisions(&f);
    let err = f.core.experiment_create(&f.project.id, spec(&[("x", "no-such-branch")], 3)).unwrap_err();
    assert!(err.to_string().contains("no-such-branch"), "{err}");
    assert!(f.core.experiment_create(&f.project.id, spec(&[("x", "--upload-pack=evil")], 3)).is_err());
    assert!(f.core.experiment_create(&f.project.id, spec(&[], 3)).is_err());
    assert!(f.core.experiment_list(&f.project.id).unwrap().is_empty());
    assert!(f.core.experiment_cancel("nope").is_err());
}

#[test]
fn a_running_experiment_cannot_be_started_twice_and_a_crash_is_recovered() {
    let f = fixture();
    repo_with_revisions(&f);
    let r = f.repo.path();
    let e = f.core.experiment_create(&f.project.id, spec(&[("fast", "fast")], 3)).unwrap();
    // Simulate a crash mid-attempt: the row says running and a worktree exists on disk.
    let running = f.core.store.exp_begin_attempt(&e.id, &serde_json::json!({})).unwrap();
    let err = f.core.experiment_run(&e.id).unwrap_err();
    assert!(err.to_string().contains("already running"), "{err}");
    let wt = f.core.cfg.home.join("experiments").join(&e.id).join(running.attempt.to_string()).join("v");
    std::fs::create_dir_all(wt.parent().unwrap()).unwrap();
    git::run(r, &["worktree", "add", "--detach", &wt.to_string_lossy(), "fast"]).unwrap();
    assert_eq!(worktree_count(r), 2);

    f.core.recover().unwrap();
    let after = f.core.store.exp_get(&e.id).unwrap();
    assert_eq!(after.state, "interrupted");
    assert!(after.last_error.unwrap().contains("stopped"));
    assert_eq!(worktree_count(r), 1, "the leftover worktree is removed");
    // It can be run again afterwards.
    assert_eq!(f.core.experiment_run(&e.id).unwrap().experiment.state, "completed");
}

#[test]
fn cancelling_stops_the_attempt_and_keeps_what_ran() {
    let f = fixture();
    repo_with_revisions(&f);
    let mut s = spec(&[("fast", "fast")], 20);
    s.command = "node -e \"setTimeout(()=>require('./bench.js'),300)\"".into();
    let e = f.core.experiment_create(&f.project.id, s).unwrap();
    let core = f.core.clone();
    let id = e.id.clone();
    let h = std::thread::spawn(move || core.experiment_run(&id));
    wait_until("the experiment is running", || f.core.store.exp_get(&e.id).unwrap().state == "running" && f.core.store.exp_results(&e.id, 1).unwrap().len() >= 3);
    f.core.experiment_cancel(&e.id).unwrap();
    let d = h.join().unwrap().unwrap();
    assert_eq!(d.experiment.state, "cancelled");
    assert!(d.results.len() < 2 + 2 + 40, "stopped early: {}", d.results.len());
    assert!(!d.results.is_empty());
    assert_eq!(worktree_count(f.repo.path()), 1);
}

#[test]
fn missions_can_be_compared_by_their_uncommitted_work_which_is_frozen_at_creation() {
    let f = fixture();
    let r = f.repo.path();
    write_rev(r, 10.0, 0);
    commit_all(r, "baseline bench");
    let base = git::head_commit(r).unwrap();
    // Two agents' attempts, left uncommitted in their worktrees as missions leave them.
    let mut ids = vec![];
    for (name, score) in [("quick", 3.0), ("slow", 20.0)] {
        let m = new_mission(&f, name, PermissionProfile::Default);
        let wt = f._home.path().join("wt").join(name);
        git::worktree_add(r, &wt, &format!("forge/{name}"), &base).unwrap();
        f.core.store.set_mission_workspace(&m.id, "main", &base, &format!("forge/{name}"), &wt.to_string_lossy()).unwrap();
        write_rev(&wt, score, 0);
        ids.push((m.id, wt));
    }
    let mut s = spec(&[], 3);
    s.variants = ids.iter().zip(["quick", "slow"]).map(|((id, _), l)| VariantSpec { label: l.into(), git_ref: String::new(), mission_id: Some(id.clone()) }).collect();
    let e = f.core.experiment_create(&f.project.id, s).unwrap();
    // The agent keeps editing after the experiment was defined: the snapshot is what gets measured.
    write_rev(&ids[0].1, 99.0, 0);
    let d = f.core.experiment_run(&e.id).unwrap();
    let rep = d.experiment.report.unwrap();
    let quick = d.variants.iter().find(|v| v.label == "quick").unwrap();
    assert_eq!(quick.git_ref, format!("mission:{}", ids[0].0));
    assert_eq!(rep.winner.as_deref(), Some(quick.id.as_str()), "{rep:#?}");
    assert_eq!(rep.variants.iter().find(|v| v.label == "slow").unwrap().verdict, Verdict::Worse);
    // The snapshot is pinned by a hidden ref (safe from gc); the missions' worktrees and branches are untouched.
    assert!(git::run(r, &["rev-parse", "--verify", &format!("refs/forge/snapshots/{}", quick.commit_sha)]).is_ok());
    assert!(git::run(&ids[1].1, &["status", "--porcelain"]).unwrap().contains("bench.js"));
    assert_eq!(git::run(r, &["rev-parse", "forge/quick"]).unwrap().trim(), base);
    // A mission from another project, or without a workspace, is refused.
    let draft = new_mission(&f, "no workspace", PermissionProfile::Default);
    let mut s = spec(&[], 3);
    s.variants = vec![VariantSpec { label: "x".into(), git_ref: String::new(), mission_id: Some(draft.id) }];
    assert!(f.core.experiment_create(&f.project.id, s).unwrap_err().to_string().contains("no workspace"));
}
