use super::{MacIdentity, deadline};
use hf_application::process::IdentityPort;
use hf_domain::process::ProcessIdentity;
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub struct Controlled {
    child: Child,
    peer: Option<UnixStream>,
    reaped: bool,
}
impl Controlled {
    pub fn spawn(payload: &Path) -> Self {
        // Keep sockaddr_un below Darwin's bound even with a long TMPDIR.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let socket = std::path::PathBuf::from(format!(
            "/private/tmp/hf-q1-{}-{nonce}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        // Open before signalling readiness; retain that exact descriptor across rename.
        // No SQLite, store path lookup, descendant or inherited store descriptor is used.
        let script = "import os,socket,sys; f=open(sys.argv[2],'ab',buffering=0); s=socket.socket(socket.AF_UNIX); s.settimeout(60); s.connect(sys.argv[1]); s.sendall(b'R'); assert s.recv(1)==b'G'; f.write(b'|Q1-after-archive-only|'); os.fsync(f.fileno()); f.close(); s.sendall(b'D'); s.close()";
        let child = Command::new("/usr/bin/python3")
            .args(["-I", "-S", "-c", script])
            .arg(&socket)
            .arg(payload)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        // Install sole-wait/kill ownership before readiness can fail.
        let mut owner = Self {
            child,
            peer: None,
            reaped: false,
        };
        let until = deadline();
        let peer = loop {
            match listener.accept() {
                Ok((peer, _)) => break peer,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < until, "orphan connect deadline");
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) => panic!("orphan accept: {e}"),
            }
        };
        peer.set_nonblocking(false).unwrap();
        std::fs::remove_file(&socket).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
        peer.set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        owner.peer = Some(peer);
        owner.receive(b'R');
        owner
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    pub fn assert_alive(&mut self, identity: &ProcessIdentity) {
        assert!(self.child.try_wait().unwrap().is_none());
        assert_eq!(MacIdentity.identity(self.pid()).as_ref(), Some(identity));
    }
    fn receive(&mut self, expected: u8) {
        let mut byte = [0];
        self.peer.as_mut().unwrap().read_exact(&mut byte).unwrap();
        assert_eq!(byte, [expected]);
    }
    pub fn release_and_reap(&mut self) {
        self.peer.as_mut().unwrap().write_all(b"G").unwrap();
        self.receive(b'D');
        let status = self.reap(deadline()).expect("orphan exit deadline");
        assert!(status.success());
    }
    fn reap(&mut self, until: Instant) -> Option<std::process::ExitStatus> {
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.reaped = true;
                    return Some(status);
                }
                Ok(None) if Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                _ => return None,
            }
        }
    }
}
impl Drop for Controlled {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        // Fixture is intentionally retained on every failure, including uncertain kill/reap.
        // Never wait without a bound and never signal an unowned or recycled PID.
        match self.child.try_wait() {
            Ok(Some(_)) => {
                self.reaped = true;
                return;
            }
            Ok(None) => {
                if let Err(error) = self.child.kill() {
                    eprintln!("Q1 fixture retained: owned child kill failed: {error}");
                }
            }
            Err(error) => {
                eprintln!("Q1 fixture retained: child observation uncertain: {error}");
                return;
            }
        }
        if self.reap(deadline()).is_none() {
            eprintln!("Q1 fixture retained: owned child not reaped within cleanup deadline");
        }
    }
}
