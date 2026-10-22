//! Mission lifecycle. The transition table is the single source of truth; every
//! state change in the application goes through `can_transition`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionState {
    Draft,
    Ready,
    Preparing,
    Running,
    AwaitingApproval,
    AwaitingReview,
    Approved,
    Merged,
    Rejected,
    Failed,
    Cancelled,
    /// The agent process is gone and FORGE cannot claim it is still running.
    Interrupted,
}

use MissionState::*;

impl MissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Draft => "draft",
            Ready => "ready",
            Preparing => "preparing",
            Running => "running",
            AwaitingApproval => "awaiting_approval",
            AwaitingReview => "awaiting_review",
            Approved => "approved",
            Merged => "merged",
            Rejected => "rejected",
            Failed => "failed",
            Cancelled => "cancelled",
            Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "draft" => Draft,
            "ready" => Ready,
            "preparing" => Preparing,
            "running" => Running,
            "awaiting_approval" => AwaitingApproval,
            "awaiting_review" => AwaitingReview,
            "approved" => Approved,
            "merged" => Merged,
            "rejected" => Rejected,
            "failed" => Failed,
            "cancelled" => Cancelled,
            "interrupted" => Interrupted,
            _ => return None,
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Merged | Rejected | Cancelled)
    }

    /// States in which an agent process is expected to be alive.
    pub fn expects_live_session(self) -> bool {
        matches!(self, Preparing | Running | AwaitingApproval)
    }
}

pub fn can_transition(from: MissionState, to: MissionState) -> bool {
    matches!(
        (from, to),
        (Draft, Ready)
            | (Draft, Cancelled)
            | (Ready, Preparing)
            | (Ready, Cancelled)
            | (Preparing, Running)
            | (Preparing, Failed)
            | (Preparing, Cancelled)
            | (Preparing, Interrupted)
            | (Running, AwaitingApproval)
            | (Running, AwaitingReview)
            | (Running, Failed)
            | (Running, Cancelled)
            | (Running, Interrupted)
            | (AwaitingApproval, Running)
            | (AwaitingApproval, Failed)
            | (AwaitingApproval, Cancelled)
            | (AwaitingApproval, Interrupted)
            | (Interrupted, Preparing)
            | (Interrupted, AwaitingReview)
            | (Interrupted, Cancelled)
            | (AwaitingReview, Running)
            | (AwaitingReview, Preparing)
            | (AwaitingReview, Approved)
            | (AwaitingReview, Rejected)
            | (AwaitingReview, Cancelled)
            | (Approved, AwaitingReview)
            | (Approved, Merged)
            | (Approved, Cancelled)
            | (Failed, Ready)
            | (Failed, Preparing)
            | (Failed, AwaitingReview)
            | (Failed, Cancelled)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [MissionState; 12] = [
        Draft, Ready, Preparing, Running, AwaitingApproval, AwaitingReview, Approved, Merged,
        Rejected, Failed, Cancelled, Interrupted,
    ];

    #[test]
    fn terminal_states_have_no_exits() {
        for t in ALL.iter().filter(|s| s.is_terminal()) {
            for to in ALL {
                assert!(!can_transition(*t, to), "{t:?} -> {to:?} must be impossible");
            }
        }
    }

    #[test]
    fn happy_path_is_legal() {
        let path = [Draft, Ready, Preparing, Running, AwaitingReview, Approved, Merged];
        for w in path.windows(2) {
            assert!(can_transition(w[0], w[1]), "{:?}->{:?}", w[0], w[1]);
        }
    }

    #[test]
    fn cannot_skip_review_or_merge_unapproved() {
        assert!(!can_transition(Running, Merged));
        assert!(!can_transition(AwaitingReview, Merged));
        assert!(!can_transition(Rejected, Approved));
        assert!(!can_transition(Ready, Running), "must pass through preparing (worktree creation)");
    }

    #[test]
    fn string_roundtrip() {
        for s in ALL {
            assert_eq!(MissionState::parse(s.as_str()), Some(s));
        }
        assert_eq!(MissionState::parse("bogus"), None);
    }

    #[test]
    fn live_states_can_always_be_marked_interrupted_or_cancelled() {
        for s in ALL.iter().filter(|s| s.expects_live_session()) {
            assert!(can_transition(*s, Cancelled), "{s:?} must be cancellable");
            assert!(can_transition(*s, Interrupted), "{s:?} must be able to become interrupted");
        }
    }
}
