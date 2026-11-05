#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;
use super::{Limits, STAGE, Store};
use crate::Error;
use hf_domain::ownership::{ClaimResult, Id, PrepareResult};
use std::{
    io::Write,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use support::{
    ActorGuard, Fixture, append_grant, args_for_request, deadline, id, owner,
    process as fixture_process, raw_state, request, tag,
};

fn ready_pair(first: &mut ActorGuard, second: &mut ActorGuard) {
    let until = deadline();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let a = barrier.clone();
    let b = barrier.clone();
    let mut input_a = first.take_input();
    let mut input_b = second.take_input();
    let (send, receive) = mpsc::channel();
    let send_a = send.clone();
    thread::spawn(move || {
        a.wait();
        let result = input_a.write_all(b"GO\n").and_then(|_| input_a.flush());
        let _ = send_a.send(result);
    });
    thread::spawn(move || {
        b.wait();
        let result = input_b.write_all(b"GO\n").and_then(|_| input_b.flush());
        let _ = send.send(result);
    });
    barrier.wait();
    for _ in 0..2 {
        receive
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .unwrap()
            .unwrap();
    }
}

fn actor_result(actor: &ActorGuard) -> String {
    actor.wait_for("ACTOR_RESULT:", deadline()).unwrap()
}

#[test]
fn ownership_actor_child() {
    if std::env::var_os("HF_B1_ACTOR").is_none() {
        return;
    }
    let root = std::env::var("HF_B1_ROOT").unwrap();
    let ledger = std::env::var("HF_B1_LEDGER").unwrap();
    let action = std::env::var("HF_B1_ACTION").unwrap();
    let op = std::env::var("HF_B1_OP").unwrap().parse::<u8>().unwrap();
    let store = Store::open(
        root,
        Limits::default(),
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    if let Ok(hook) = std::env::var("HF_B1_HOOK") {
        *STAGE
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .unwrap() = Some(hook);
    }
    println!("ACTOR_READY");
    let _ = std::io::stdout().flush();
    let mut command = String::new();
    std::io::stdin().read_line(&mut command).unwrap();
    assert_eq!(command.trim(), "GO");
    let duration = std::env::var("HF_B1_DEADLINE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());
    let call_deadline = if let Some(milliseconds) = duration {
        let instant_deadline = Instant::now() + Duration::from_millis(milliseconds);
        let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
            + Duration::from_millis(milliseconds);
        println!("ACTOR_DEADLINE_NS:{}", wall.as_nanos());
        instant_deadline
    } else {
        deadline()
    };
    let result = actor_call(&store, &action, op, &ledger, call_deadline);
    println!("ACTOR_RESULT:{result}");
    let _ = std::io::stdout().flush();
}

fn actor_call(store: &Store, action: &str, op: u8, ledger: &str, until: Instant) -> String {
    let operation = id(op);
    let generation = Id::new(store.identity().generation.clone()).unwrap();
    match action {
        "prepare" => {
            let session = env_tag("HF_B1_SESSION");
            let reservation = env_tag("HF_B1_RESERVATION");
            let attempt = env_tag("HF_B1_ATTEMPT");
            match store.ownership_prepare(&request(store, op, session, reservation, attempt), until)
            {
                Ok(PrepareResult::Prepared(_)) => "Prepared".to_owned(),
                Ok(PrepareResult::Existing(_)) => "Existing".to_owned(),
                Err(error) => error_name(error),
            }
        }
        "claim" => {
            let revision = env_tag("HF_B1_REVISION") as u64;
            let claimant = owner(env_tag("HF_B1_OWNER"));
            match store.ownership_claim(&operation, &generation, revision, &claimant, until) {
                Ok(ClaimResult::Granted(_)) => {
                    append_grant(std::path::Path::new(ledger), &operation, &claimant.owner);
                    "Granted".to_owned()
                }
                Ok(ClaimResult::NoGrant(_)) => "NoGrant".to_owned(),
                Err(error) => error_name(error),
            }
        }
        "cancel" => match store.ownership_cancel(&operation, &generation, 0, until) {
            Ok(_) => "Cancelled".to_owned(),
            Err(error) => error_name(error),
        },
        "accept" => match store
            .ownership_observe(&operation, until)
            .unwrap()
            .unwrap()
            .fence()
            .map(|fence| store.ownership_record_acceptance(&fence, until))
        {
            Some(Ok(_)) => "Accepted".to_owned(),
            Some(Err(error)) => error_name(error),
            None => "NoFence".to_owned(),
        },
        _ => "InvalidAction".to_owned(),
    }
}

fn env_tag(name: &str) -> u8 {
    std::env::var(name).unwrap().parse().unwrap()
}
fn error_name(error: Error) -> String {
    match error {
        Error::Busy => "Busy",
        Error::DeadlineExceeded => "DeadlineExceeded",
        Error::OwnershipConflict => "OwnershipConflict",
        Error::StaleOwnership => "StaleOwnership",
        other => return format!("Other:{other:?}"),
    }
    .to_owned()
}
fn is_busy(error: &Error) -> bool {
    matches!(error, Error::Busy | Error::DeadlineExceeded)
        || matches!(error, Error::Sqlite(rusqlite::Error::SqliteFailure(code, _)) if matches!(code.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))
}

mod commit;
mod races;
