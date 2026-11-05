use super::*;
use hf_platform::fixture::{append_effect, qualify_ledger};
use hf_storage::{Limits, Mutation, Store};
pub(super) fn run(channel: PrivateIo, until: Instant) -> io::Result<()> {
    let Frame::Start(start) = receive(&channel)? else {
        return Err(invalid());
    };
    channel.expect_eof()?;
    let parent = root_parent(&start.root)?;
    if qualify_ledger(Path::new(&start.root))? != start.ledger {
        return Err(invalid());
    }
    let actual = self_identity()?;
    // Fixed diagnostic effect only. This proves the inherited-lock boundary using
    // an independent real helper; fixture DML does not grant ownership authority.
    {
        let identity = map(hf_storage::probe_with_scratch(
            &start.root,
            until,
            &parent,
            hf_platform::lease::shared(),
        ))?
        .ok_or_else(invalid)?;
        if identity.generation != start.fence.generation.as_str()
            || start.operation != start.fence.operation
        {
            return Err(invalid());
        }
        let store = map(Store::open(
            &start.root,
            Limits::default(),
            until,
            hf_platform::lease::shared(),
        ))?;
        if store.identity() != &identity {
            return Err(invalid());
        }
        let stored = map(store.ownership_observe(&start.operation, until))?.ok_or_else(invalid)?;
        if stored.phase != hf_domain::ownership::Phase::SpawnObserved
            || stored.fence().as_ref() != Some(&start.fence)
            || stored.child.as_ref() != Some(&actual)
            || stored.cancelled
        {
            return Err(invalid());
        }
        map(store.apply_batch(
            &[Mutation::Insert {
                key: format!("r03-b2-probe:{}", start.operation.as_str()),
                value: actual.pid().to_string().into_bytes(),
            }],
            until,
        ))?;
    }
    append_effect(
        Path::new(&start.root),
        start.ledger,
        &start.operation,
        &actual,
        until,
    )?;
    send(
        &channel,
        &Frame::EffectDone {
            operation: start.operation,
            process: actual,
        },
    )?;
    hf_platform::fixture::write_all(
        &std::io::stdout(),
        hf_platform::fixture::FIXED_STDOUT,
        until,
    )?;
    hf_platform::fixture::write_all(
        &std::io::stderr(),
        hf_platform::fixture::FIXED_STDERR,
        until,
    )
}
