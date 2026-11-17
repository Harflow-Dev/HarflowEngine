//! Controlled original child blocks on a private socket, independent of stdin EOF.
use super::super::super::{
    actor::{Ack, Actor},
    retention::Resources,
};
use super::super::{RestoreDirectory, owner, support};
use hf_application::{
    lease::{LeaseError, LeaseMode},
    process::IdentityPort,
};
use hf_domain::{
    capture::CaptureBinding,
    fixture::{Gate, Stage},
    owner_runtime::{StorageCommand, StorageRefusal, Stream},
};
use hf_platform::process::{
    CaptureOrigin, MacIdentity, RetainedOwnedChild, SpawnSpec, current_parent_identity,
};
use std::{
    cell::{Cell, RefCell},
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    rc::Rc,
    time::Instant,
};

type Observer = Box<dyn FnMut(&Resources, Option<&Actor>)>;
thread_local! { static OBSERVER: RefCell<Option<Observer>> = RefCell::new(None); }

pub(super) fn observe(resources: &Resources, actor: Option<&Actor>) {
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow_mut().as_mut() {
            observer(resources, actor);
        }
    });
}
struct ClearObserver;
impl Drop for ClearObserver {
    fn drop(&mut self) {
        OBSERVER.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

#[test]
fn actual_writer_and_root_remain_owned_until_controlled_child_and_reader_settle() {
    for cause in [StorageRefusal::Fenced, StorageRefusal::CommitLost] {
        exercise(cause);
    }
}

fn exercise(cause: StorageRefusal) {
    let _serial = crate::fixture::test_support::case_guard();
    let scratch = support::Scratch::new(current_parent_identity().unwrap());
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(&scratch.path).unwrap();
    let _restore = RestoreDirectory(previous);
    let mut owner = prepare_owner(&scratch);
    let (agent, peer) = controlled_child(&scratch.path);
    let identity = agent.registered_identity().unwrap().clone();
    owner.resources.agent = Some(agent);
    exchange(
        &mut owner.actor,
        StorageCommand::RecordChild(identity),
        Ack::RecordChild,
    );
    let origin =
        CaptureOrigin::from_spawn_observed(owner.actor.coordinator.snapshot().unwrap()).unwrap();
    owner.resources.reader = Some(
        owner
            .resources
            .agent
            .as_mut()
            .unwrap()
            .start_private_fixture_capture_reader(origin, support::deadline(8))
            .unwrap(),
    );
    exchange(&mut owner.actor, StorageCommand::CreateRaw, Ack::CreateRaw);
    exchange(&mut owner.actor, StorageCommand::Accept, Ack::Accept);
    begin_capture(&mut owner.actor);
    exchange(&mut owner.actor, append(), Ack::Append);
    super::inject(&scratch, cause);
    exchange(&mut owner.actor, append(), Ack::Refused(cause));
    assert_no_commands(&mut owner.actor);
    let observed = install_observer(&scratch.root, peer, cause);
    let _clear = ClearObserver;
    owner.resources.retain(Some(&mut owner.actor));
    assert!(
        observed.get(),
        "observer saw actual live original child and writer before release"
    );
    assert!(owner.resources.settled(Some(&owner.actor)));
    assert!(owner.resources.reader_joined && owner.resources.reader_fifo_reconciled);
    let final_value = owner
        .resources
        .reader
        .as_ref()
        .unwrap()
        .view()
        .final_observation()
        .unwrap();
    assert!(final_value.progress.stdout.eof && final_value.progress.stderr.eof);
    assert_eq!(final_value.progress.stdout.read, 65536);
    assert_eq!(final_value.progress.stderr.read, 65536);
    assert!(!final_value.stopped && !final_value.limited);
    assert!(!owner.actor.coordinator.is_poisoned());
    super::super::assert_root_hold_lifetime(&scratch, owner);
}

fn prepare_owner(scratch: &support::Scratch) -> owner::OwnerLoop {
    let mut prepared = scratch.prepared(
        support::self_identity(),
        Gate {
            stage: Stage::CaptureAfterPublish,
            path: scratch
                .path
                .join("unused.sock")
                .to_string_lossy()
                .into_owned(),
        },
    );
    prepared.gate = None;
    let mut owner =
        owner::start_at(prepared, support::deadline(8), &support::built_scaffold()).unwrap();
    assert_eq!(receive(&mut owner.actor), Ack::Bootstrap);
    exchange(&mut owner.actor, StorageCommand::Claim, Ack::Claim);
    owner
}

fn begin_capture(actor: &mut Actor) {
    let snapshot = actor.coordinator.snapshot().unwrap();
    let raw = actor.coordinator.raw_files().unwrap();
    let binding = CaptureBinding {
        fence: snapshot.fence().unwrap(),
        child: snapshot.child.clone().unwrap(),
        stdout: raw.stdout.clone(),
        stderr: raw.stderr.clone(),
    };
    exchange(
        actor,
        StorageCommand::CaptureBegin(Box::new(binding)),
        Ack::CaptureBegin,
    );
}
fn append() -> StorageCommand {
    StorageCommand::Append {
        stream: Stream::Stdout,
        bytes: vec![b'x'],
        drain: None,
    }
}
fn assert_no_commands(actor: &mut Actor) {
    let sequence = actor.coordinator.next_sequence();
    let capture = actor.coordinator.capture().cloned();
    for command in [
        append(),
        StorageCommand::KnownLoss,
        StorageCommand::Finish(hf_domain::process::ProcessExit::Code(0)),
        StorageCommand::Close,
    ] {
        assert!(actor.submit(command).is_err());
    }
    assert_eq!(actor.coordinator.next_sequence(), sequence);
    assert_eq!(actor.coordinator.capture(), capture.as_ref());
}
fn exchange(actor: &mut Actor, command: StorageCommand, expected: Ack) {
    actor.submit(command).unwrap();
    assert_eq!(receive(actor), expected);
}
fn receive(actor: &mut Actor) -> Ack {
    let until = support::deadline(5);
    loop {
        if let Some(ack) = actor.step().unwrap() {
            return ack;
        }
        assert!(Instant::now() < until, "actual writer ack deadline");
    }
}

fn controlled_child(path: &Path) -> (RetainedOwnedChild, UnixStream) {
    let socket = path.join("controlled-child.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let script = "import socket,sys; s=socket.socket(socket.AF_UNIX); s.settimeout(5.0); s.connect(sys.argv[1]); s.sendall(b'R'); assert s.recv(1)==b'G'; sys.stdout.buffer.write(b'o'*65536); sys.stdout.buffer.flush(); sys.stderr.buffer.write(b'e'*65536); sys.stderr.buffer.flush()";
    let child = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: "/usr/bin/python3".into(),
            args: vec![
                "-I".into(),
                "-S".into(),
                "-c".into(),
                script.into(),
                socket.into_os_string(),
            ],
            cwd: path.to_path_buf(),
            environment: Vec::new(),
            capture_limit: 16384,
        },
        &MacIdentity,
        support::deadline(3),
    )
    .unwrap();
    (child, accept_ready(&listener))
}

fn accept_ready(listener: &UnixListener) -> UnixStream {
    let until = support::deadline(3);
    let mut peer = loop {
        match listener.accept() {
            Ok((peer, _)) => break peer,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::yield_now()
            }
            Err(error) => panic!("controlled child gate: {error}"),
        }
        assert!(Instant::now() < until);
    };
    peer.set_nonblocking(true).unwrap();
    let ready_until = support::deadline(2);
    let mut ready = [0];
    loop {
        match peer.read(&mut ready) {
            Ok(1) => break,
            Ok(_) => panic!("controlled child closed before ready"),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                assert!(
                    Instant::now() < ready_until,
                    "controlled child ready deadline"
                );
                std::thread::yield_now();
            }
            Err(error) => panic!("controlled child ready: {error}"),
        }
    }
    assert_eq!(ready, [b'R']);
    peer.set_nonblocking(false).unwrap();
    peer.set_write_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    peer
}

fn install_observer(root: &Path, mut peer: UnixStream, cause: StorageRefusal) -> Rc<Cell<bool>> {
    let observed = Rc::new(Cell::new(false));
    let result = observed.clone();
    let root = root.to_path_buf();
    OBSERVER.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move |resources, actor| {
            if observed.get() {
                return;
            }
            let actor = actor.unwrap();
            assert_eq!(actor.coordinator.storage_refusal(), Some(cause));
            assert!(
                !actor.io.channels_complete(),
                "refused writer input remains open before child settlement"
            );
            assert!(!actor.coordinator.is_poisoned());
            for child in [&resources.agent, &resources.writer] {
                let identity = child.as_ref().unwrap().registered_identity().unwrap();
                assert_eq!(
                    MacIdentity.identity(identity.pid()).as_ref(),
                    Some(identity)
                );
            }
            assert!(!resources.reader_joined);
            let lease = hf_platform::lease::shared().acquire(
                &root.join("store.lock"),
                LeaseMode::Exclusive,
                false,
                support::deadline(1),
            );
            assert!(matches!(lease, Err(LeaseError::Busy)));
            observed.set(true);
            peer.write_all(b"G").unwrap();
        }))
    });
    result
}
