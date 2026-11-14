use super::*;
use hf_platform::fixture::{append_effect, qualify_ledger};
use hf_storage::{Limits, Mutation, Store};
pub(super) fn run(
    channel: PrivateIo,
    until: Instant,
    saturation: bool,
    protocol: bool,
) -> io::Result<()> {
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
        let identity = map(hf_storage::probe_with_deadline(
            &start.root,
            until,
            hf_platform::lease::shared(),
        ))?
        .ok_or_else(invalid)?;
        if identity.generation != start.fence.generation.as_str()
            || start.operation != start.fence.operation
        {
            return Err(invalid());
        }
        let store = map(Store::open_with(
            &start.root,
            Limits::default(),
            until,
            hf_platform::lease::shared(),
            crate::durability::suppressed(),
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
        validate_protocol(protocol, &stored.request, &start, &parent, until)?;
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
    if protocol {
        return hf_platform::fixture::write_all(&std::io::stdout(), b"{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"fixed-protocol\"}\n{\"type\":\"assistant\",\"session_id\":\"fixed-protocol\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"fixed helper text\"}]}}\n{\"type\":\"result\",\"subtype\":\"success\",\"session_id\":\"fixed-protocol\",\"is_error\":false,\"result\":\"fixed helper text\"}\n", until);
    }
    if saturation {
        return saturated_output(until);
    }
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

fn saturated_output(until: Instant) -> io::Result<()> {
    let bytes = [b'x'; 16384];
    for length in [16384, 16384, 16384, 16384, 8192] {
        hf_platform::fixture::write_all(&std::io::stdout(), &bytes[..length], until)?;
        hf_platform::fixture::write_all(&std::io::stderr(), &bytes[..length], until)?;
    }
    Ok(())
}

fn validate_protocol(
    protocol: bool,
    request: &hf_domain::ownership::PreparedRequest,
    start: &EffectStart,
    parent: &Path,
    until: Instant,
) -> io::Result<()> {
    if protocol {
        let launch = hf_interfaces::prepared_launch::decode(request, &hf_storage::Sha256Digest)
            .map_err(|_| invalid())?;
        let pin = hf_platform::prepared_launch::observe_binary(
            &std::env::current_exe()?,
            &hf_storage::Sha256Digest,
            until,
        )
        .map_err(|_| invalid())?;
        if launch.binary != pin
            || Path::new(&launch.cwd) != parent
            || launch.ledger != start.ledger
            || map(hf_storage::RootHold::open(
                &start.root,
                until,
                hf_platform::lease::shared(),
            ))?
            .source_stamp()
                != &launch.source
        {
            return Err(invalid());
        }
    } else if request.payload.starts_with(b"HFPL") {
        return Err(invalid());
    }
    Ok(())
}
