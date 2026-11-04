#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use hf_domain::ownership::ReleaseProof;

#[test]
fn independent_actors_prepare_one_session_and_claim_once_with_external_grant_ledger() {
    let fixture = Fixture::new("actors");
    let seed = fixture.store();
    let a = request(&seed, 2, 3, 4, 5);
    let b = request(&seed, 6, 3, 7, 8);
    drop(seed);
    let prepared = prepare_once(&fixture, &a, &b);
    claim_once(&fixture, &prepared);
}

fn prepare_once(
    fixture: &Fixture,
    a: &hf_domain::ownership::PreparedRequest,
    b: &hf_domain::ownership::PreparedRequest,
) -> hf_domain::ownership::PreparedRequest {
    let mut first = ActorGuard::spawn(
        &fixture.root(),
        &fixture.ledger(),
        "prepare",
        tag(&a.operation),
        &args_for_request(a),
    );
    let mut second = ActorGuard::spawn(
        &fixture.root(),
        &fixture.ledger(),
        "prepare",
        tag(&b.operation),
        &args_for_request(b),
    );
    ready_pair(&mut first, &mut second);
    let outcomes = [actor_result(&first), actor_result(&second)];
    let until = deadline();
    first.finish(until).unwrap();
    second.finish(until).unwrap();
    assert_eq!(
        outcomes.iter().filter(|v| v.ends_with("Prepared")).count(),
        1,
        "{outcomes:?}"
    );
    if outcomes[0].ends_with("Busy") || outcomes[1].ends_with("Busy") {
        let loser = if outcomes[0].ends_with("Busy") { a } else { b };
        let store = fixture.open_store();
        assert!(matches!(
            store.ownership_prepare(loser, deadline()),
            Err(Error::OwnershipConflict)
        ));
    } else {
        assert!(
            outcomes.iter().any(|v| v.ends_with("OwnershipConflict")),
            "{outcomes:?}"
        );
    }
    let raw = fixture.raw_readonly();
    assert_eq!(
        raw.query_row(
            "SELECT count(*) FROM ownership_records WHERE session=?1 AND phase!=4",
            [a.session.as_str()],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    if raw_state(&raw, &a.operation).is_some() {
        a.clone()
    } else {
        b.clone()
    }
}

fn claim_once(fixture: &Fixture, prepared: &hf_domain::ownership::PreparedRequest) {
    let op = tag(&prepared.operation);
    let args_one = [("HF_B1_REVISION", "0".into()), ("HF_B1_OWNER", "20".into())];
    let args_two = [("HF_B1_REVISION", "0".into()), ("HF_B1_OWNER", "30".into())];
    let mut one = ActorGuard::spawn(&fixture.root(), &fixture.ledger(), "claim", op, &args_one);
    let mut two = ActorGuard::spawn(&fixture.root(), &fixture.ledger(), "claim", op, &args_two);
    ready_pair(&mut one, &mut two);
    let claims = [actor_result(&one), actor_result(&two)];
    let until = deadline();
    one.finish(until).unwrap();
    two.finish(until).unwrap();
    assert_eq!(
        claims.iter().filter(|v| v.ends_with("Granted")).count(),
        1,
        "{claims:?}"
    );
    if claims.iter().any(|v| v.ends_with("Busy")) {
        let store = fixture.open_store();
        assert!(matches!(
            store
                .ownership_claim(
                    &prepared.operation,
                    &prepared.generation,
                    0,
                    &owner(30),
                    deadline()
                )
                .unwrap(),
            ClaimResult::NoGrant(_)
        ));
    } else {
        assert!(claims.iter().any(|v| v.ends_with("NoGrant")), "{claims:?}");
    }
    assert_eq!(fixture.ledger_lines().len(), 1);
    assert!(!fixture.ledger().starts_with(fixture.root()));
    let replay = fixture.open_store();
    assert!(matches!(
        replay
            .ownership_claim(
                &prepared.operation,
                &prepared.generation,
                1,
                &owner(30),
                deadline()
            )
            .unwrap(),
        ClaimResult::NoGrant(_)
    ));
    assert_eq!(fixture.ledger_lines().len(), 1);
    let raw = fixture.raw_readonly();
    assert_eq!(
        raw_state(&raw, &prepared.operation).map(|v| (v.0, v.1)),
        Some((1, 1))
    );
    assert_eq!(
        raw.query_row(
            "SELECT count(*) FROM ownership_records WHERE phase=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn independent_claim_cancel_race_has_one_cas_winner_and_revision_bound_release() {
    let fixture = Fixture::new("cancel-race");
    let seed = fixture.store();
    let req = request(&seed, 2, 3, 4, 5);
    seed.ownership_prepare(&req, deadline()).unwrap();
    drop(seed);
    let outcomes = cancel_race(&fixture, &req);
    let raw = fixture.raw_readonly();
    let state = raw_state(&raw, &req.operation).unwrap();
    assert!(
        (state.0 == 1 && state.1 == 1 && !state.2) || (state.0 == 0 && state.1 == 1 && state.2),
        "{state:?} {outcomes:?}"
    );
    if state.0 == 1 {
        claim_won(&fixture, &req, &raw, &outcomes);
    } else {
        cancel_won(&fixture, &req, &raw, &outcomes);
    }
}

fn cancel_race(fixture: &Fixture, req: &hf_domain::ownership::PreparedRequest) -> [String; 2] {
    let claim_args = [("HF_B1_REVISION", "0".into()), ("HF_B1_OWNER", "20".into())];
    let mut claim = ActorGuard::spawn(
        &fixture.root(),
        &fixture.ledger(),
        "claim",
        tag(&req.operation),
        &claim_args,
    );
    let mut cancel = ActorGuard::spawn(
        &fixture.root(),
        &fixture.ledger(),
        "cancel",
        tag(&req.operation),
        &[],
    );
    ready_pair(&mut claim, &mut cancel);
    let mut outcomes = [actor_result(&claim), actor_result(&cancel)];
    let until = deadline();
    claim.finish(until).unwrap();
    cancel.finish(until).unwrap();
    retry_busy_claim(fixture, req, &mut outcomes[0]);
    retry_busy_cancel(fixture, req, &mut outcomes[1]);
    outcomes
}

fn retry_busy_claim(
    fixture: &Fixture,
    req: &hf_domain::ownership::PreparedRequest,
    outcome: &mut String,
) {
    if !outcome.ends_with("Busy") {
        return;
    }
    let owner = owner(20);
    let store = fixture.open_store();
    *outcome = match store.ownership_claim(&req.operation, &req.generation, 0, &owner, deadline()) {
        Ok(ClaimResult::Granted(_)) => {
            append_grant(&fixture.ledger(), &req.operation, &owner.owner);
            "ActorResult:Granted".into()
        }
        Ok(ClaimResult::NoGrant(_)) => "ActorResult:NoGrant".into(),
        Err(Error::StaleOwnership) => "ActorResult:StaleOwnership".into(),
        Err(error) => panic!("known-Busy claim retry had unexpected outcome: {error:?}"),
    };
}

fn retry_busy_cancel(
    fixture: &Fixture,
    req: &hf_domain::ownership::PreparedRequest,
    outcome: &mut String,
) {
    if !outcome.ends_with("Busy") {
        return;
    }
    let store = fixture.open_store();
    *outcome = match store.ownership_cancel(&req.operation, &req.generation, 0, deadline()) {
        Ok(_) => "ActorResult:Cancelled".into(),
        Err(Error::StaleOwnership) => "ActorResult:StaleOwnership".into(),
        Err(error) => panic!("known-Busy cancel retry had unexpected outcome: {error:?}"),
    };
}

fn claim_won(
    fixture: &Fixture,
    req: &hf_domain::ownership::PreparedRequest,
    raw: &rusqlite::Connection,
    outcomes: &[String; 2],
) {
    assert!(
        outcomes.iter().any(|v| v.ends_with("Granted")),
        "{outcomes:?}"
    );
    assert!(
        outcomes.iter().any(|v| v.ends_with("StaleOwnership")),
        "{outcomes:?}"
    );
    assert_eq!(fixture.ledger_lines().len(), 1);
    let store = fixture.open_store();
    let snapshot = store
        .ownership_observe(&req.operation, deadline())
        .unwrap()
        .unwrap();
    let rejected = store.ownership_release(
        &req.operation,
        &req.generation,
        snapshot.revision,
        snapshot.fence().as_ref(),
        &ReleaseProof::Unclaimed { evidence: id(70) },
        deadline(),
    );
    assert!(matches!(rejected, Err(Error::InvalidOwnership)));
    assert_eq!(
        raw_state(raw, &req.operation).map(|v| (v.0, v.1, v.2)),
        Some((1, 1, false))
    );
}

fn cancel_won(
    fixture: &Fixture,
    req: &hf_domain::ownership::PreparedRequest,
    raw: &rusqlite::Connection,
    outcomes: &[String; 2],
) {
    assert!(
        outcomes.iter().any(|v| v.ends_with("Cancelled")),
        "{outcomes:?}"
    );
    assert!(
        outcomes
            .iter()
            .any(|v| v.ends_with("NoGrant") || v.ends_with("StaleOwnership")),
        "{outcomes:?}"
    );
    assert!(fixture.ledger_lines().is_empty());
    let store = fixture.open_store();
    assert!(matches!(
        store.ownership_release(
            &req.operation,
            &req.generation,
            0,
            None,
            &ReleaseProof::Unclaimed { evidence: id(70) },
            deadline()
        ),
        Err(Error::StaleOwnership)
    ));
    store
        .ownership_release(
            &req.operation,
            &req.generation,
            1,
            None,
            &ReleaseProof::Unclaimed { evidence: id(70) },
            deadline(),
        )
        .unwrap();
    assert_eq!(
        raw_state(raw, &req.operation).map(|v| (v.0, v.1, v.2)),
        Some((4, 2, true))
    );
}

#[test]
fn sqlite_busy_claim_deadline_leaves_no_queued_right_after_lock_release() {
    let fixture = Fixture::new("sqlite-busy");
    let store = fixture.store();
    let req = request(&store, 2, 3, 4, 5);
    store.ownership_prepare(&req, deadline()).unwrap();
    let writer = fixture.raw_writer();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let result = store.ownership_claim(
        &req.operation,
        &req.generation,
        0,
        &owner(20),
        Instant::now() + Duration::from_millis(120),
    );
    assert!(result.as_ref().is_err_and(is_busy), "{result:?}");
    let observer = fixture.raw_readonly();
    assert_eq!(
        raw_state(&observer, &req.operation).map(|v| (v.0, v.1)),
        Some((0, 0))
    );
    drop(observer);
    writer.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        raw_state(&fixture.raw_readonly(), &req.operation).map(|v| (v.0, v.1)),
        Some((0, 0))
    );
    assert!(matches!(
        store
            .ownership_claim(&req.operation, &req.generation, 0, &owner(20), deadline())
            .unwrap(),
        ClaimResult::Granted(_)
    ));
    assert_eq!(
        raw_state(&fixture.raw_readonly(), &req.operation).map(|v| (v.0, v.1)),
        Some((1, 1))
    );
}
