//! Agents overview: every agent session FORGE started recently, what is running now, and provider-reported usage per provider.
//! Read-only. Usage figures are whatever the provider reported for sessions FORGE launched; they are not a bill and do not
//! include the user's own sessions outside FORGE.

use super::Core;
use crate::error::Result;
use crate::store::AgentSession;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub session: AgentSession,
    pub mission_title: String,
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub provider: String,
    pub sessions: u32,
    pub live: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub turns: u64,
    /// Sum of the provider's own cost estimates, over the sessions that reported one.
    pub cost_usd_estimate: Option<f64>,
    /// How many sessions reported usage at all (the rest contribute nothing to the totals).
    pub sessions_with_usage: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentsOverview {
    pub since: String,
    pub days: u32,
    pub sessions: Vec<SessionRow>,
    pub usage: Vec<ProviderUsage>,
}

fn num(v: &serde_json::Value, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_f64()).filter(|x| x.is_finite() && *x >= 0.0).map(|x| x as u64).unwrap_or(0)
}

/// Totals per provider. Pure, so it can be tested without a database.
pub fn usage_by_provider(rows: &[SessionRow]) -> Vec<ProviderUsage> {
    let mut by: BTreeMap<String, ProviderUsage> = BTreeMap::new();
    for r in rows {
        let s = &r.session;
        let u = by.entry(s.provider.clone()).or_insert_with(|| ProviderUsage { provider: s.provider.clone(), ..Default::default() });
        u.sessions += 1;
        if matches!(s.state.as_str(), "starting" | "live") {
            u.live += 1;
        }
        if let Some(v) = &s.usage {
            u.sessions_with_usage += 1;
            u.input_tokens += num(v, "inputTokens");
            u.output_tokens += num(v, "outputTokens");
            u.cache_read_tokens += num(v, "cacheReadTokens");
            u.cache_creation_tokens += num(v, "cacheCreationTokens");
            u.turns += num(v, "numTurns");
            if let Some(c) = v.get("costUsdEstimate").and_then(|c| c.as_f64()).filter(|c| c.is_finite() && *c >= 0.0) {
                u.cost_usd_estimate = Some(u.cost_usd_estimate.unwrap_or(0.0) + c);
            }
        }
    }
    by.into_values().collect()
}

impl Core {
    /// Sessions of the last `days` days (plus anything still running), newest first, and usage per provider over them.
    pub fn agents_overview(&self, days: u32) -> Result<AgentsOverview> {
        let days = days.clamp(1, 365);
        let since = (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let sessions: Vec<SessionRow> = self.store.sessions_since(&since, 500)?.into_iter().map(|(session, mission_title, project_id)| SessionRow { session, mission_title, project_id }).collect();
        let usage = usage_by_provider(&sessions);
        Ok(AgentsOverview { since, days, sessions, usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(provider: &str, state: &str, usage: Option<serde_json::Value>) -> SessionRow {
        SessionRow {
            session: AgentSession { id: "s".into(), mission_id: "m".into(), provider: provider.into(), provider_session_id: None, state: state.into(), pid: None, started_at: "t".into(), ended_at: None, end_reason: None, usage, executes_on: "agent_host".into() },
            mission_title: "t".into(),
            project_id: "p".into(),
        }
    }

    #[test]
    fn usage_is_summed_per_provider_and_cost_only_where_reported() {
        let rows = vec![
            row("claude", "ended", Some(json!({"inputTokens": 100, "outputTokens": 10, "cacheReadTokens": 5, "cacheCreationTokens": 1, "costUsdEstimate": 0.25, "numTurns": 3}))),
            row("claude", "live", Some(json!({"inputTokens": 50, "outputTokens": 5, "cacheReadTokens": 0, "cacheCreationTokens": 0}))),
            row("claude", "lost", None),
            row("codex", "ended", Some(json!({"inputTokens": 7, "outputTokens": 2, "cacheReadTokens": 1, "cacheCreationTokens": 0, "costUsdEstimate": "NaN"}))),
        ];
        let u = usage_by_provider(&rows);
        assert_eq!(u.len(), 2);
        let c = &u[0];
        assert_eq!((c.provider.as_str(), c.sessions, c.live, c.sessions_with_usage), ("claude", 3, 1, 2));
        assert_eq!((c.input_tokens, c.output_tokens, c.cache_read_tokens, c.turns), (150, 15, 5, 3));
        assert_eq!(c.cost_usd_estimate, Some(0.25));
        assert_eq!(u[1].cost_usd_estimate, None, "a malformed or missing estimate is not invented");
    }
}
