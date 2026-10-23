//! ACP missions end to end: the core starts the engine, the engine starts a registered ACP agent (a scripted one built on
//! the official SDK), and every permission/file request the agent makes goes through FORGE policy and approvals.

use super::*;

fn fixture_agent(f: &Fixture) -> crate::acp::AcpAgent {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../packages/engine/test/fixtures/acp-agent.mjs");
    let script = dunce::canonicalize(script).unwrap();
    let node = which::which("node").expect("node on PATH");
    f.core.acp_agent_register("scripted", &node.to_string_lossy(), &[script.to_string_lossy().into_owned()]).unwrap()
}

fn acp_mission(f: &Fixture, objective: &str) -> Mission {
    f.core.create_mission(&f.project.id, objective, None, "acp", Some("scripted"), PermissionProfile::Default).unwrap()
}

fn agent_result(f: &Fixture, m: &Mission) -> Value {
    let text: String = f.core.store.events(&m.id, 0, 1000).unwrap().into_iter().filter(|e| e.kind == "agent.text").map(|e| e.payload["text"].as_str().unwrap_or("").to_string()).collect();
    let at = text.find("RESULT ").expect("result line");
    serde_json::from_str(&text[at + 7..]).unwrap()
}

#[test]
fn an_acp_agents_edit_is_approved_by_a_human_and_written_by_forge_into_the_worktree() {
    let f = fixture();
    fixture_agent(&f);
    let m = acp_mission(&f, "ACP:edit please");
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("approval", || !pending(&f, &m.id).is_empty());
    let a = pending(&f, &m.id).remove(0);
    assert_eq!(a.tool, "Edit");
    f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref()).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert_eq!(std::fs::read_to_string(wt.join("FORGE_ACP.md")).unwrap(), "# written through FORGE\n");
    assert!(!f.repo.path().join("FORGE_ACP.md").exists(), "main checkout untouched");
    assert_eq!(agent_result(&f, &m)["write"], "ok");
    let started = f.core.store.events(&m.id, 0, 1000).unwrap().into_iter().find(|e| e.kind == "session.started").unwrap();
    assert_eq!(started.payload["acp"]["agentInfo"]["name"], "scripted-acp");
    assert_eq!(started.payload["acp"]["protocolVersion"], 1);
    assert_eq!(pending(&f, &m.id).len(), 0, "the write after an approved edit did not ask again");
}

#[test]
fn policy_denies_an_acp_agents_escape_and_push_without_asking() {
    let f = fixture();
    fixture_agent(&f);
    let m = acp_mission(&f, "ACP:escape ACP:exec");
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    let r = agent_result(&f, &m);
    assert_eq!(r["edit"], "ro", "write outside the worktree");
    assert_eq!(r["exec"], "ro", "git push");
    assert!(f.core.store.pending_approvals(Some(&m.id)).unwrap().is_empty());
    assert!(f.core.store.approvals_for_mission(&m.id).unwrap().iter().all(|a| a.state == "auto_denied"));
    assert_eq!(f.core.store.approvals_for_mission(&m.id).unwrap().len(), 2);
}

#[test]
fn acp_missions_need_a_registered_agent() {
    let f = fixture();
    let err = f.core.create_mission(&f.project.id, "x", None, "acp", Some("nobody"), PermissionProfile::Default).unwrap_err().to_string();
    assert!(err.contains("no registered ACP agent"), "{err}");
    assert!(f.core.acp_agent_register("bad name", "node", &[]).is_err());
    let a = fixture_agent(&f);
    assert!(Path::new(&a.command).is_absolute(), "pinned to an absolute path");
    assert_eq!(f.core.store.audit_count("acp.register").unwrap(), 1);
    f.core.acp_agent_remove(&a.id).unwrap();
    assert!(f.core.create_mission(&f.project.id, "x", None, "acp", Some("scripted"), PermissionProfile::Default).is_err());
}

/// Protocol review P1: an ACP agent runs MCP tools itself, so FORGE cannot enforce a tool allow-list for it. Servers
/// authorized *with* an allow-list are withheld from ACP missions; unrestricted ones are passed.
#[test]
fn acp_missions_do_not_receive_mcp_servers_whose_allow_list_they_could_bypass() {
    let f = fixture();
    fixture_agent(&f);
    std::fs::write(f.repo.path().join(".mcp.json"), r#"{"mcpServers": {"open": {"command": "open-mcp"}, "narrow": {"command": "narrow-mcp"}}}"#).unwrap();
    for (name, tools) in [("open", vec![]), ("narrow", vec!["search".to_string()])] {
        let p = f.core.mcp_preview(&f.project.id, name).unwrap();
        f.core.mcp_authorize(&f.project.id, name, &p.definition_sha256, &tools).unwrap();
    }
    let m = acp_mission(&f, "ACP:report");
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    assert_eq!(agent_result(&f, &m)["mcp"], json!(["open"]));
}
