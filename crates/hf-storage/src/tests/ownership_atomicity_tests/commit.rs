#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

#[test]
fn lost_claim_ack_and_acceptance_ack_are_resolved_as_committed_without_peer_delivery_claim() {
    for (action, hook, initial) in [
        ("claim", "ownership_before_commit", 0),
        ("claim", "ownership_after_commit", 0),
        ("accept", "ownership_before_commit", 2),
        ("accept", "ownership_after_commit", 2),
    ] {
        let fixture = Fixture::new("ack-boundary");
        let store = fixture.store();
        let req = request(&store, 2, 3, 4, 5);
        store.ownership_prepare(&req, deadline()).unwrap();
        if initial == 2 {
            let ClaimResult::Granted(claimed) = store
                .ownership_claim(&req.operation, &req.generation, 0, &owner(20), deadline())
                .unwrap()
            else {
                panic!("seed claim")
            };
            store
                .ownership_record_spawn(
                    &claimed.fence().unwrap(),
                    &fixture_process(808),
                    deadline(),
                )
                .unwrap();
        }
        drop(store);
        let mut args = vec![("HF_B1_HOOK", hook.to_owned())];
        if action == "claim" {
            args.extend([("HF_B1_REVISION", "0".into()), ("HF_B1_OWNER", "50".into())]);
        }
        let mut actor = ActorGuard::spawn(&fixture.root(), &fixture.ledger(), action, 2, &args);
        actor.go().unwrap();
        actor
            .wait_for(&format!("STAGE:{hook}"), deadline())
            .unwrap();
        let raw = fixture.raw_readonly();
        let expected = match (action, hook) {
            ("claim", "ownership_before_commit") => (0, 0, false),
            ("claim", _) => (1, 1, false),
            ("accept", "ownership_before_commit") => (2, 2, false),
            _ => (3, 3, false),
        };
        assert_eq!(
            raw_state(&raw, &req.operation).map(|v| (v.0, v.1, v.2)),
            Some(expected)
        );
        if action == "accept" {
            assert_eq!(
                fixture.raw_child(&req.operation).as_deref(),
                Some("808:12:34:abcdef01-2345-6789-abcd-ef0123456789")
            );
        }
        actor.kill_and_reap().unwrap();
        drop(raw);
        if hook == "ownership_after_commit" && action == "claim" {
            assert!(
                fixture.ledger_lines().is_empty(),
                "commit without returned Granted acknowledgment is not recorded as an external grant"
            );
            let reopened = fixture.open_store();
            assert!(matches!(
                reopened
                    .ownership_claim(&req.operation, &req.generation, 1, &owner(52), deadline())
                    .unwrap(),
                ClaimResult::NoGrant(_)
            ));
            assert!(fixture.ledger_lines().is_empty());
        }
    }
}

#[test]
fn expired_deadline_after_update_rolls_back_the_staged_revision() {
    let fixture = Fixture::new("rollback-stage");
    let store = fixture.store();
    let req = request(&store, 2, 3, 4, 5);
    store.ownership_prepare(&req, deadline()).unwrap();
    drop(store);
    let mut actor = ActorGuard::spawn(
        &fixture.root(),
        &fixture.ledger(),
        "cancel",
        2,
        &[
            ("HF_B1_HOOK", "ownership_before_commit".into()),
            ("HF_B1_DEADLINE_MS", "500".into()),
        ],
    );
    actor.go().unwrap();
    let deadline_line = actor.wait_for("ACTOR_DEADLINE_NS:", deadline()).unwrap();
    let stage = actor
        .wait_for("STAGE:ownership_before_commit", deadline())
        .unwrap();
    let target = deadline_line
        .split(':')
        .nth(1)
        .unwrap()
        .parse::<u128>()
        .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    if target > now {
        let (send, receive) = mpsc::channel::<()>();
        assert!(matches!(
            receive.recv_timeout(Duration::from_nanos((target - now) as u64)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(send);
    }
    assert_eq!(stage, "STAGE:ownership_before_commit");
    actor.go().unwrap();
    assert_eq!(actor_result(&actor), "ACTOR_RESULT:DeadlineExceeded");
    actor.finish(deadline()).unwrap();
    let raw = fixture.raw_readonly();
    assert_eq!(
        raw_state(&raw, &req.operation).map(|v| (v.0, v.1, v.2, v.3)),
        Some((0, 0, false, None))
    );
    assert!(fixture.ledger_lines().is_empty());
}
