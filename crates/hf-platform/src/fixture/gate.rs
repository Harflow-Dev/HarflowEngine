use super::{read_frame, require_eof, write_all};
use hf_domain::{
    fixture::{Gate, Stage},
    ownership::Id,
};
use nix::{
    fcntl::{FcntlArg, FdFlag, OFlag, fcntl},
    poll::PollFlags,
    sys::socket::{self, AddressFamily, SockFlag, SockType, UnixAddr, sockopt},
};
use std::{
    io,
    os::{
        fd::AsRawFd,
        unix::{fs::FileTypeExt, net::UnixStream},
    },
    time::Instant,
};
/// Constructor-selected fixture barrier. The sole connection is dropped on every
/// return path; callers invoke only after closing Store and before spawn.
pub struct FixtureGate {
    selected: Option<Gate>,
}
impl FixtureGate {
    pub fn new(selected: Option<Gate>) -> Self {
        Self { selected }
    }
    pub fn reach(
        &self,
        stage: Stage,
        operation: &Id,
        nonce: &Id,
        deadline: Instant,
    ) -> io::Result<()> {
        let Some(gate) = &self.selected else {
            return Ok(());
        };
        if gate.stage != stage {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("fixture gate deadline"));
        }
        let socket = connect(&gate.path, deadline)?;
        let packet = gate_packet(stage, operation, nonce);
        write_all(&socket, &packet, deadline)?;
        let echoed = read_frame(&socket, deadline)?;
        if echoed != packet {
            return Err(io::Error::other("fixture gate binding"));
        }
        require_eof(&socket, deadline)
    }
}
/// A gate peer echoes exactly this bounded context and closes its write half.
pub fn gate_packet(stage: Stage, operation: &Id, nonce: &Id) -> Vec<u8> {
    let mut body = b"HFG2\x01".to_vec();
    body.push(match stage {
        Stage::ParentBeforePrepare => 1,
        Stage::ParentAfterPrepare => 2,
        Stage::ParentBeforeSpawn => 3,
        Stage::ParentAfterOwnerSpawn => 4,
        Stage::OwnerAfterClaim => 5,
        Stage::OwnerAfterChildSpawn => 6,
        Stage::OwnerAfterObserved => 7,
        Stage::OwnerBeforeAck => 8,
    });
    body.extend_from_slice(operation.as_str().as_bytes());
    body.extend_from_slice(nonce.as_str().as_bytes());
    let mut packet = 70_u32.to_be_bytes().to_vec();
    packet.extend(body);
    packet
}

fn connect(path: &str, deadline: Instant) -> io::Result<UnixStream> {
    if Instant::now() >= deadline {
        return Err(io::Error::other("fixture gate deadline"));
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket() {
        return Err(io::Error::other("fixture gate endpoint"));
    }
    let fd = socket::socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )?;
    fcntl(&fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
    fcntl(&fd, FcntlArg::F_SETFL(OFlag::O_NONBLOCK))?;
    let address = UnixAddr::new(path)?;
    match socket::connect(fd.as_raw_fd(), &address) {
        Ok(()) => {}
        Err(nix::errno::Errno::EINPROGRESS) => {
            super::channel::ready(&fd, PollFlags::POLLOUT, deadline)?;
            let error = socket::getsockopt(&fd, sockopt::SocketError)?;
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
        }
        Err(e) => return Err(e.into()),
    }
    if Instant::now() >= deadline {
        return Err(io::Error::other("fixture gate deadline"));
    }
    Ok(UnixStream::from(fd))
}
