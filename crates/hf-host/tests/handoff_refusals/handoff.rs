#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use super::process::{ChildGuard, NONCE, OP, OWNER, Seed, create_owner, deadline, id, run_oracle};
use hf_domain::{
    fixture::{FixtureFault, Frame, Invitation, Prepared},
    process::ProcessIdentity,
};
use hf_interfaces::fixture::{decode, encode};
use std::{fs, time::Instant};

#[derive(Clone, Copy)]
pub(crate) enum BindingFault {
    PreparedNonce,
    ChangedPayload,
    ChangedGeneration,
    StaleRevision,
    WrongPhysicalOwner,
}
#[derive(Clone, Copy)]
pub(crate) enum MalformedInput {
    Truncated,
    InvalidMagic,
    Oversized,
    Trailing,
    Eof,
}

struct Owner {
    child: ChildGuard,
    actual: ProcessIdentity,
    invitation: Invitation,
    until: Instant,
}
fn start_owner(seed: &Seed) -> Owner {
    let until = deadline(8);
    let child = create_owner(until, &seed.scratch.0);
    let Frame::Hello(actual) = decode(&child.0.read_fixture_frame(until).expect("owner hello"))
        .expect("decode owner hello")
    else {
        panic!("expected actual owner hello");
    };
    assert_eq!(child.0.registered_identity(), Some(&actual));
    let invitation = Invitation {
        operation: seed.request.operation.clone(),
        generation: seed.generation.clone(),
        digest: seed.request.digest.clone(),
        owner: id(OWNER),
        nonce: id(NONCE),
        process: actual.clone(),
        revision: 0,
    };
    Owner {
        child,
        actual,
        invitation,
        until,
    }
}
fn send_invitation(owner: &mut Owner, invitation: &Invitation) {
    let bytes = encode(&Frame::Invitation(invitation.clone())).expect("encode invitation");
    owner
        .child
        .0
        .write_fixture_frame(&bytes, owner.until)
        .expect("write invitation");
    if invitation.process == owner.actual {
        assert_eq!(
            decode(
                &owner
                    .child
                    .0
                    .read_fixture_frame(owner.until)
                    .expect("owner ready")
            )
            .expect("decode owner ready"),
            Frame::Ready(invitation.clone())
        );
    }
}
fn prepared(seed: &Seed, invitation: Invitation) -> Prepared {
    Prepared {
        invitation,
        request: seed.request.clone(),
        root: seed.root.to_string_lossy().into_owned(),
        gate: None,
        ledger: seed.ledger,
        fault: FixtureFault::None,
    }
}
fn finish_refusal(owner: &mut Owner) {
    owner.child.0.close_stdin();
    owner
        .child
        .0
        .fixture_eof(owner.until)
        .expect("owner closes private output on refusal");
    owner.child.finish(owner.until, 2);
}

pub(crate) fn mismatched_refuses(seed: &Seed, fault: BindingFault) {
    let mut owner = start_owner(seed);
    let mut invitation = owner.invitation.clone();
    let expects_ready = !matches!(fault, BindingFault::WrongPhysicalOwner);
    match fault {
        BindingFault::StaleRevision => invitation.revision = 1,
        BindingFault::WrongPhysicalOwner => {
            invitation.process = ProcessIdentity::new(
                owner.actual.pid(),
                owner.actual.start_seconds() + 1,
                owner.actual.start_microseconds(),
                owner.actual.boot_uuid().into(),
            )
            .expect("valid physical mismatch")
        }
        _ => {}
    }
    send_invitation(&mut owner, &invitation);
    let mut handoff = prepared(seed, invitation.clone());
    match fault {
        BindingFault::PreparedNonce => {
            handoff.invitation.nonce = id("88888888888888888888888888888888")
        }
        BindingFault::ChangedPayload => handoff.request.payload.push(0x5a),
        BindingFault::ChangedGeneration => {
            handoff.request.generation = id("99999999999999999999999999999999")
        }
        BindingFault::StaleRevision | BindingFault::WrongPhysicalOwner => {}
    }
    if expects_ready {
        let bytes = encode(&Frame::Prepared(handoff)).expect("encode prepared negative");
        owner
            .child
            .0
            .write_fixture_frame(&bytes, owner.until)
            .expect("send prepared request");
    }
    finish_refusal(&mut owner);
}

pub(crate) fn malformed_refuses(seed: &Seed, fault: MalformedInput) {
    let mut owner = start_owner(seed);
    let invitation = owner.invitation.clone();
    send_invitation(&mut owner, &invitation);
    if !matches!(fault, MalformedInput::Eof) {
        let valid = encode(&Frame::Prepared(prepared(seed, owner.invitation.clone())))
            .expect("encode valid prepared frame");
        let bytes = match fault {
            MalformedInput::Truncated => valid[..valid.len() - 1].to_vec(),
            MalformedInput::InvalidMagic => {
                let mut b = valid;
                b[4] = b'X';
                b
            }
            MalformedInput::Oversized => 24_573_u32.to_be_bytes().to_vec(),
            MalformedInput::Trailing => {
                let mut b = valid;
                b.push(0);
                b
            }
            MalformedInput::Eof => Vec::new(),
        };
        owner
            .child
            .0
            .write_fixture_frame(&bytes, owner.until)
            .expect("send malformed request bytes");
    }
    finish_refusal(&mut owner);
}

pub(crate) fn assert_unchanged(seed: &Seed, before: &str) {
    let after = run_oracle(&seed.root, OP, &seed.scratch.0);
    assert_eq!(after, before, "refusal changed SQLite ownership state");
    assert_eq!(
        fs::read(seed.scratch.0.join("r03-b2.effects")).expect("read effect ledger"),
        Vec::<u8>::new()
    );
}
