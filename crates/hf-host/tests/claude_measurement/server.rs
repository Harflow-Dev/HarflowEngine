#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::owner_runtime::DigestPort;
use hf_interfaces::backend_measurement::{anthropic_text_response, decode_anthropic_request};
use hf_platform::process::RetainedOwnedChild;
#[path = "http.rs"]
mod http;
use std::{
    io,
    net::TcpListener,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const EXCHANGE_LIMIT: Duration = Duration::from_secs(120);
const FAKE_KEY: &str = "harflow-fake-fixture-key";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ServerOutcome {
    NoRequest,
    PreflightOnly,
    Completed,
}

pub(super) struct FixtureServer {
    port: u16,
    ipv6_guard: TcpListener,
    _ipv4_guard: TcpListener,
    stop: Option<Sender<()>>,
    worker: Option<JoinHandle<Result<ServerOutcome, String>>>,
    post_seen: Option<Receiver<()>>,
    post_release: Option<Sender<()>>,
}
impl FixtureServer {
    pub(super) fn start(nonce: String) -> io::Result<Self> {
        Self::start_with_preflight_only(nonce, false)
    }
    pub(super) fn start_preflight_only(nonce: String) -> io::Result<Self> {
        Self::start_with_preflight_only(nonce, true)
    }
    pub(super) fn start_owner_protocol(nonce: String) -> io::Result<Self> {
        Self::start_with_post_hold(nonce)
    }
    fn start_with_preflight_only(nonce: String, preflight_only: bool) -> io::Result<Self> {
        Self::start_mode(nonce, preflight_only, None)
    }
    fn start_with_post_hold(nonce: String) -> io::Result<Self> {
        let (seen_tx, seen_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        Self::start_mode(nonce, false, Some((seen_tx, release_rx))).map(|mut server| {
            server.post_seen = Some(seen_rx);
            server.post_release = Some(release_tx);
            server
        })
    }
    fn start_mode(
        nonce: String,
        preflight_only: bool,
        post_hold: Option<(Sender<()>, Receiver<()>)>,
    ) -> io::Result<Self> {
        let (listener, ipv6_guard, port) = reserve_loopback_pair()?;
        let ipv4_guard = listener.try_clone()?;
        let (stop, stopped) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = serve(listener, stopped, nonce, preflight_only, post_hold);
            eprintln!("phase=fixture-server-worker-return result={result:?}");
            result
        });
        Ok(Self {
            port,
            ipv6_guard,
            _ipv4_guard: ipv4_guard,
            stop: Some(stop),
            worker: Some(worker),
            post_seen: None,
            post_release: None,
        })
    }
    pub(super) fn port(&self) -> u16 {
        self.port
    }
    pub(super) fn no_ipv6_connection(&self) -> bool {
        matches!(self.ipv6_guard.accept(), Err(ref error) if error.kind() == io::ErrorKind::WouldBlock)
    }
    pub(super) fn wait_for_validated_post_or_parent_exit(
        &mut self,
        deadline: Instant,
        parent: &mut RetainedOwnedChild,
    ) -> Result<(), String> {
        let receiver = self.post_seen.take().ok_or("POST hold is not enabled")?;
        loop {
            match receiver.try_recv() {
                Ok(()) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    return Err("validated POST observer disconnected".into());
                }
                Err(TryRecvError::Empty) => (),
            }
            let exit = parent.try_wait().map_err(show)?;
            if exit != hf_domain::process::ProcessExit::Unobserved {
                return Err(format!("parent settled before validated POST: {exit:?}"));
            }
            if Instant::now() >= deadline {
                return Err("validated POST observation deadline expired".into());
            }
            thread::yield_now();
        }
    }
    pub(super) fn release_post(&mut self) -> Result<(), String> {
        let release = self
            .post_release
            .take()
            .ok_or("POST hold already released")?;
        release
            .send(())
            .map_err(|_| "POST release receiver exited".into())
    }
    pub(super) fn stop_and_join(&mut self) -> ServerOutcome {
        self.stop_and_join_result()
            .expect("fixture server accepted only its bounded protocol")
    }
    pub(super) fn stop_and_join_result(&mut self) -> Result<ServerOutcome, String> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let worker = self.worker.take().ok_or("fixture worker already joined")?;
        let result = join_bounded(worker).and_then(|outcome| outcome);
        eprintln!("phase=fixture-server-stop result={result:?}");
        result
    }
}

fn reserve_loopback_pair() -> io::Result<(TcpListener, TcpListener, u16)> {
    for _ in 0..16 {
        let ipv4 = TcpListener::bind(("127.0.0.1", 0))?;
        ipv4.set_nonblocking(true)?;
        let port = ipv4.local_addr()?.port();
        match TcpListener::bind(("::1", port)) {
            Ok(ipv6) => {
                ipv6.set_nonblocking(true)?;
                return Ok((ipv4, ipv6, port));
            }
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => drop(ipv4),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "could not reserve an exclusive IPv4/IPv6 loopback pair within 16 attempts",
    ))
}
impl Drop for FixtureServer {
    fn drop(&mut self) {
        if let Some(release) = self.post_release.take() {
            let _ = release.send(());
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let result = join_bounded(worker).and_then(|outcome| outcome);
            eprintln!("phase=fixture-server-drop result={result:?}");
        }
    }
}

pub(super) struct DeniedListener {
    listener: TcpListener,
    port: u16,
}
impl DeniedListener {
    pub(super) fn bind(allowed_port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        if port == allowed_port {
            return Err(io::Error::other("fixture endpoints collided"));
        }
        Ok(Self { listener, port })
    }
    pub(super) fn port(&self) -> u16 {
        self.port
    }
    pub(super) fn no_connection(&self) -> bool {
        matches!(self.listener.accept(), Err(ref error) if error.kind() == io::ErrorKind::WouldBlock)
    }
}

fn serve(
    listener: TcpListener,
    stop: Receiver<()>,
    nonce: String,
    preflight_only: bool,
    post_hold: Option<(Sender<()>, Receiver<()>)>,
) -> Result<ServerOutcome, String> {
    let end = Instant::now() + EXCHANGE_LIMIT;
    let mut preflight = false;
    let mut completed = false;
    loop {
        if Instant::now() >= end {
            return Err("fixture listener deadline expired".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_millis(250)))
                    .map_err(show)?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .map_err(show)?;
                let request = http::read_request(&mut stream, end)?;
                log_request(&request);
                if is_preflight_request(preflight, completed, &request, &nonce) {
                    http::reply(&mut stream, b"preflight-ok")?;
                    preflight = true;
                } else if is_fixture_request(preflight_only, preflight, completed, &request, &nonce)
                {
                    if request.headers.get("x-api-key").map(String::as_str) != Some(FAKE_KEY) {
                        return Err("fixture POST lacks exact fake authentication".into());
                    }
                    let parsed = decode_anthropic_request(&request.body, &nonce)
                        .map_err(|error| format!("fixture codec rejected request: {error:?}"))?;
                    let body = anthropic_text_response(&parsed.model, &nonce)
                        .map_err(|error| format!("fixture response refused: {error:?}"))?;
                    if let Some((observed, release)) = &post_hold {
                        observed.send(()).map_err(show)?;
                        release
                            .recv_timeout(end.saturating_duration_since(Instant::now()))
                            .map_err(|error| {
                                format!("validated POST hold release failed: {error}")
                            })?;
                    }
                    http::reply(&mut stream, &body)?;
                    completed = true;
                } else {
                    return Err(
                        "fixture listener rejected unexpected method, route, order or count".into(),
                    );
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if stopped(&stop)? {
                    return Ok(if completed {
                        ServerOutcome::Completed
                    } else if preflight {
                        ServerOutcome::PreflightOnly
                    } else {
                        ServerOutcome::NoRequest
                    });
                }
                thread::yield_now();
            }
            Err(error) => return Err(show(error)),
        }
    }
}

fn log_request(request: &http::Request) {
    let digest = hf_storage::Sha256Digest.digest_parts(&[&request.body]);
    let method: String = request.method.chars().take(512).collect();
    let path: String = request.path.chars().take(512).collect();
    let fake_auth_match = request.headers.get("x-api-key").map(String::as_str) == Some(FAKE_KEY);
    eprintln!(
        "phase=fixture-request method={method:?} path={path:?} body-length={} body-sha256={} fake-auth-match={fake_auth_match}",
        request.body.len(),
        digest.as_str()
    );
}

fn is_preflight_request(
    preflight: bool,
    completed: bool,
    request: &http::Request,
    nonce: &str,
) -> bool {
    !preflight
        && !completed
        && request.method == "GET"
        && request.path == format!("/{nonce}/preflight")
        && request.body.is_empty()
}

fn is_fixture_request(
    preflight_only: bool,
    preflight: bool,
    completed: bool,
    request: &http::Request,
    nonce: &str,
) -> bool {
    !preflight_only
        && preflight
        && !completed
        && request.method == "POST"
        && (request.path == format!("/{nonce}/v1/messages")
            || request.path == format!("/{nonce}/v1/messages?beta=true"))
}

fn stopped(stop: &Receiver<()>) -> Result<bool, String> {
    match stop.try_recv() {
        Ok(()) | Err(TryRecvError::Disconnected) => Ok(true),
        Err(TryRecvError::Empty) => Ok(false),
    }
}

fn join_bounded<T: Send + 'static>(worker: JoinHandle<T>) -> Result<T, String> {
    let end = Instant::now() + EXCHANGE_LIMIT + Duration::from_secs(2);
    while !worker.is_finished() && Instant::now() < end {
        thread::yield_now();
    }
    if !worker.is_finished() {
        std::process::abort();
    }
    worker
        .join()
        .map_err(|_| "fixture server thread panicked".into())
}
fn show(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_only_route_accepts_only_the_nonce_get_before_reply() {
        let nonce = "offline-test-nonce";
        let get = request("GET", format!("/{nonce}/preflight"));
        let post = request("POST", format!("/{nonce}/v1/messages"));
        assert!(is_preflight_request(false, false, &get, nonce));
        assert!(!is_fixture_request(true, true, false, &post, nonce));
        assert!(is_fixture_request(false, true, false, &post, nonce));
    }

    fn request(method: &str, path: String) -> http::Request {
        http::Request {
            method: method.into(),
            path,
            headers: Default::default(),
            body: Vec::new(),
        }
    }
}

#[cfg(test)]
#[path = "server/ownership_tests.rs"]
mod ownership_tests;
