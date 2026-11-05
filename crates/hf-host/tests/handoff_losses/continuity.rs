#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::support::*;
use hf_domain::fixture::{Frame, Gate, ReportKind, Stage};
use std::fs;

#[test]
fn prepared_eof_owner_continues_after_parent_loss_at_claim_spawn_and_observation() {
    for (index, stage, expected_phase) in [
        (8, Stage::OwnerAfterClaim, 1),
        (9, Stage::OwnerAfterChildSpawn, 1),
        (10, Stage::OwnerAfterObserved, 2),
    ] {
        let fixture = Fixture::new();
        let (listener, socket) = fixture.bind(index);
        let mut plan = fixture.plan();
        plan.gate = Some(Gate {
            stage,
            path: socket.to_string_lossy().into_owned(),
        });
        let mut parent = fixture.parent(plan);
        let mut peer = gate_peer(&listener, stage, OP, NONCE);
        let staged = oracle(&fixture.root, OP, &fixture.scratch);
        assert_eq!(number(&staged, "phase"), Some(expected_phase), "{staged}");
        let owner = oracle_process(&staged, "owner_process");
        assert!(
            identity_is_current(&owner),
            "owner row must match its current OS identity"
        );
        assert_empty(&fixture.effects);
        kill_direct(&mut parent);
        echo_gate(&mut peer, stage, OP, NONCE);
        wait_identity_gone(owner.pid());
        let completed = oracle(&fixture.root, OP, &fixture.scratch);
        assert_eq!(
            number(&completed, "phase"),
            Some(3),
            "prepared EOF authorized completion after parent loss: {completed}"
        );
        assert_eq!(oracle_process(&completed, "owner_process"), owner);
        let child = oracle_process(&completed, "child");
        let entries = fs::read_to_string(&fixture.effects).expect("fixed effect ledger");
        assert_eq!(entries.lines().count(), 1, "{entries}");
        assert!(
            entries.starts_with(&format!("{OP}|{}|", child.pid())),
            "effect ledger must bind accepted child: {entries}"
        );
        let generation = oracle_generation(&completed);
        let mut replay_plan = fixture.plan_for(request_for(OP, SESSION, &generation));
        replay_plan.initialize = false;
        let mut replay = fixture.parent(replay_plan);
        assert!(matches!(
            response(&replay.0),
            Frame::Report {
                kind: ReportKind::Existing,
                ..
            }
        ));
        finish_protocol(&mut replay);
        assert_eq!(
            fs::read_to_string(&fixture.effects)
                .expect("ledger after replay")
                .lines()
                .count(),
            1
        );
    }
}
