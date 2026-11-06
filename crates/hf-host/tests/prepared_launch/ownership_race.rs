use super::support::*;
use hf_domain::ownership::{ClaimResult, OwnerClaim, PrepareResult, PreparedRequest};
use hf_storage::{Limits, Store};
use std::{fs, path::Path, time::Instant};

pub(super) fn concurrent_claims(
    root: &Path,
    request: &PreparedRequest,
) -> Vec<(OwnerClaim, ClaimResult)> {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let stores = [
        Store::open(
            root,
            Limits::default(),
            deadline(5),
            hf_platform::lease::shared(),
        )
        .unwrap(),
        Store::open(
            root,
            Limits::default(),
            deadline(5),
            hf_platform::lease::shared(),
        )
        .unwrap(),
    ];
    let mut joins = Vec::new();
    for ((owner_id, process_id), store) in [('6', 601), ('8', 602)].into_iter().zip(stores) {
        let operation = request.operation.clone();
        let generation = request.generation.clone();
        let barrier = barrier.clone();
        joins.push(std::thread::spawn(move || {
            let owner = OwnerClaim {
                owner: id(owner_id),
                nonce: id(if owner_id == '6' { '7' } else { '9' }),
                process: fake_process(process_id),
            };
            barrier.wait();
            let until = deadline(5);
            loop {
                match store.ownership_claim(&operation, &generation, 0, &owner, until) {
                    Ok(result) => return (owner, result),
                    Err(hf_storage::Error::Busy) if Instant::now() < until => {
                        std::thread::yield_now()
                    }
                    Err(error) => panic!("actual claim contender failed: {error:?}"),
                }
            }
        }));
    }
    joins.into_iter().map(|join| join.join().unwrap()).collect()
}
pub(super) fn assert_existing_cannot_reclaim(
    root: &Path,
    scratch: &Path,
    request: &PreparedRequest,
    outcomes: &[(OwnerClaim, ClaimResult)],
) {
    let store = Store::open(
        root,
        Limits::default(),
        deadline(3),
        hf_platform::lease::shared(),
    )
    .unwrap();
    assert!(matches!(
        store.ownership_prepare(request, deadline(3)).unwrap(),
        PrepareResult::Existing(_)
    ));
    let loser = outcomes
        .iter()
        .find_map(|(owner, result)| matches!(result, ClaimResult::NoGrant(_)).then_some(owner))
        .unwrap();
    assert!(matches!(
        store
            .ownership_claim(
                &request.operation,
                &request.generation,
                0,
                loser,
                deadline(3)
            )
            .unwrap(),
        ClaimResult::NoGrant(_)
    ));
    assert!(fs::read(scratch.join("r03-b2.effects")).unwrap().is_empty());
}
