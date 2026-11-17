#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::{id, invitation};
use hf_application::task_runtime::InvalidIntent;
use hf_domain::{
    ownership::OwnerClaim,
    process::ProcessIdentity,
    task_runtime::{WriterBootstrap, WriterReady},
};
use hf_interfaces::task_runtime::{
    WRITER_PAYLOAD_BYTES, decode_writer_bootstrap, decode_writer_ready, encode_writer_bootstrap,
    encode_writer_ready,
};

fn writer_process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(
        pid,
        1_768_000_010,
        u64::from(pid),
        "01234567-89ab-cdef-0123-456789abcdef".into(),
    )
    .unwrap()
}

fn writer_owner() -> OwnerClaim {
    OwnerClaim {
        owner: id(10),
        nonce: id(11),
        process: writer_process(4600),
    }
}

#[test]
fn writer_bootstrap_codec_enforces_root_process_and_payload_bounds() {
    let invitation = invitation();
    let bootstrap = WriterBootstrap {
        root: "/workspace/harflow".into(),
        invitation: invitation.clone(),
        owner: writer_owner(),
        writer: writer_process(4601),
    };
    let bytes = encode_writer_bootstrap(&bootstrap).unwrap();
    assert!(bytes.len() <= WRITER_PAYLOAD_BYTES);
    assert_eq!(decode_writer_bootstrap(&bytes).unwrap(), bootstrap);
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(decode_writer_bootstrap(&trailing), Err(InvalidIntent));
    assert_eq!(
        decode_writer_bootstrap(&vec![0; WRITER_PAYLOAD_BYTES + 1]),
        Err(InvalidIntent)
    );

    let mut bad_root = bootstrap.clone();
    bad_root.root = "/workspace/../other".into();
    assert_eq!(encode_writer_bootstrap(&bad_root), Err(InvalidIntent));
    let mut same_process = bootstrap;
    same_process.writer = same_process.owner.process.clone();
    assert_eq!(encode_writer_bootstrap(&same_process), Err(InvalidIntent));
}

#[test]
fn writer_ready_roundtrip_preserves_binding_and_requires_first_revisions() {
    let invitation = invitation();
    let ready = WriterReady {
        binding: invitation.binding.clone(),
        writer: writer_process(4601),
        owner: writer_owner(),
        revision: 1,
        ownership_revision: 1,
    };
    let bytes = encode_writer_ready(&ready).unwrap();
    assert!(bytes.len() <= WRITER_PAYLOAD_BYTES);
    let decoded = decode_writer_ready(&bytes).unwrap();
    assert_eq!(decoded, ready);
    assert_eq!(decoded.binding, invitation.binding);
    let mut trailing = bytes;
    trailing.push(1);
    assert_eq!(decode_writer_ready(&trailing), Err(InvalidIntent));

    let mut wrong_lifecycle_revision = ready.clone();
    wrong_lifecycle_revision.revision = 2;
    assert_eq!(
        encode_writer_ready(&wrong_lifecycle_revision),
        Err(InvalidIntent)
    );
    let mut wrong_ownership_revision = ready;
    wrong_ownership_revision.ownership_revision = 0;
    assert_eq!(
        encode_writer_ready(&wrong_ownership_revision),
        Err(InvalidIntent)
    );
}
