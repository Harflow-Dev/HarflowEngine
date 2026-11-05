use crate::{Error, Result, ownership_codec as codec};
use hf_domain::{capture::*, ownership::*};
pub(crate) fn encode(b: &CaptureBinding) -> Result<String> {
    if b.stdout == b.stderr
        || b.stdout.inode == 0
        || b.stderr.inode == 0
        || b.fence.revision > i64::MAX as u64
    {
        return Err(Error::InvalidOwnership);
    }
    let encoded = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|1",
        b.fence.operation.as_str(),
        b.fence.generation.as_str(),
        b.fence.attempt.as_str(),
        b.fence.owner.owner.as_str(),
        b.fence.owner.nonce.as_str(),
        codec::process(&b.fence.owner.process),
        b.fence.revision,
        codec::process(&b.child),
        b.stdout.device,
        b.stdout.inode,
        b.stderr.device,
        b.stderr.inode
    );
    if encoded.len() > 512 {
        return Err(Error::LimitExceeded);
    }
    Ok(encoded)
}
pub(crate) fn decode(s: &str) -> Result<CaptureBinding> {
    let p: Vec<_> = s.split('|').collect();
    if p.len() != 13 || p[12] != "1" {
        return Err(Error::InvalidOwnership);
    }
    let id = |i: usize| codec::id(p[i].to_owned());
    let number = |i: usize| p[i].parse::<u64>().map_err(|_| Error::InvalidOwnership);
    let b = CaptureBinding {
        fence: Fence {
            operation: id(0)?,
            generation: id(1)?,
            attempt: id(2)?,
            owner: OwnerClaim {
                owner: id(3)?,
                nonce: id(4)?,
                process: codec::decode_process(p[5])?,
            },
            revision: number(6)?,
        },
        child: codec::decode_process(p[7])?,
        stdout: FileIdentity {
            device: number(8)?,
            inode: number(9)?,
        },
        stderr: FileIdentity {
            device: number(10)?,
            inode: number(11)?,
        },
    };
    if encode(&b)? != s {
        return Err(Error::InvalidOwnership);
    }
    Ok(b)
}
