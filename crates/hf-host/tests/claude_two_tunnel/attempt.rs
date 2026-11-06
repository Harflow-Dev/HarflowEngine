use super::*;
pub(super) struct Attempt {
    pub(super) root: FixtureRoot,
    pub(super) nonce: String,
    pub(super) profile: String,
    pub(super) proxy: Option<TwoTunnelProxy>,
    denied: TcpListener,
    selected: identity::VendorSelection,
    outside: canary::Canary,
    owner: canary::Canary,
}

impl Attempt {
    pub(super) fn from_root(
        root: FixtureRoot,
        address: std::net::SocketAddr,
        until: Instant,
        selected: identity::VendorSelection,
        mode: Mode,
    ) -> Self {
        let nonce = identity::nonce_from_root(root.root());
        let proxy = match mode {
            Mode::TwoTunnel => TwoTunnelProxy::start(address, &nonce, until),
            Mode::ReplacementWave | Mode::HandshakeDiagnostic => {
                TwoTunnelProxy::start_replacement_wave(address, &nonce, until)
            }
        }
        .expect("exclusive loopback proxy pair");
        let profile = hf_platform::qualification::live::live_profile(&root, proxy.port())
            .expect("recorded profile plus credential denial");
        let denied = TcpListener::bind(("127.0.0.1", 0)).expect("denied-port control listener");
        denied.set_nonblocking(true).unwrap();
        let outside = canary::Canary::create(
            root.root()
                .parent()
                .unwrap()
                .join(format!("{nonce}-live-read-canary")),
            b"O",
        );
        let owner =
            canary::Canary::create(root.protected().join("owner-control-sentinel"), b"owner");
        let attempt = Self {
            outside,
            owner,
            root,
            nonce,
            profile,
            proxy: Some(proxy),
            denied,
            selected,
        };
        evidence::instance(&attempt);
        attempt
    }
    pub(super) fn preflight_and_version(&self, until: Instant) {
        assert_eq!(
            self.selected.expected_hash,
            "677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d"
        );
        self.preflight(until);
        let version_until = until.min(Instant::now() + Duration::from_secs(15));
        let before = pins::observe(version_until);
        let command = self.command();
        let version = actors::run(
            &self.root,
            &self.profile,
            Path::new(&command.executable),
            &["--version".into()],
            environment(&command.env),
            version_until,
            8192,
        );
        actors::require_clean(&version, "pinned-version");
        assert!(
            version.stdout == b"2.1.318 (Claude Code)\n",
            "exact pinned version required"
        );
        assert!(version.stderr.is_empty(), "version stderr forbidden");
        pins::unchanged(&before, version_until);
        evidence::version(self);
    }
    fn preflight(&self, until: Instant) {
        let until = until.min(Instant::now() + Duration::from_secs(5));
        self.spawn_control(until);
        self.verify_controls();
        let outside = self.outside.path();
        let inside = self.root.runtime().join("inside-canary");
        let owner = self.owner.path();
        fs::write(&inside, b"I").unwrap();
        let port = self.proxy.as_ref().unwrap().port().to_string();
        let denied = self.denied.local_addr().unwrap().port().to_string();
        let args = [
            "-I",
            "-S",
            HELPER,
            "--owner-control-denial",
            owner.to_str().unwrap(),
            outside.to_str().unwrap(),
            inside.to_str().unwrap(),
            &port,
            &denied,
            &self.nonce,
        ]
        .map(OsString::from);
        let env = environment(&[(
            "HOME".into(),
            self.root.runtime().to_string_lossy().into_owned(),
        )]);
        let result = actors::run(
            &self.root,
            &self.profile,
            Path::new(PYTHON),
            &args,
            env,
            until,
            4096,
        );
        actors::require_clean(&result, "same-profile-preflight");
        self.verify_controls();
        self.require_denied_quiet();
        assert!(
            String::from_utf8_lossy(&result.stdout).contains(
                "fork=1 posix_spawn=1 read=1 inside=1 allowed=1 denied_port=1 owner_write_denied=1"
            ),
            "same-profile preflight failed"
        );
    }
    pub(super) fn command(&self) -> hf_backends::claude_measurement::ClaudeFixtureCommand {
        live_command(
            &request(
                &self.root,
                &self.nonce,
                self.proxy.as_ref().unwrap().port(),
                &self.selected.logical,
            ),
            self.proxy.as_ref().unwrap().port(),
        )
        .unwrap()
    }
    pub(super) fn require_denied_quiet(&self) {
        let denied_quiet = matches!(self.denied.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock);
        let ipv6_quiet = self.proxy.as_ref().map(|proxy| proxy.quiet_ipv6());
        eprintln!(
            "phase=two-tunnel-denied-listeners ipv4-quiet={denied_quiet} ipv6-quiet={ipv6_quiet:?}"
        );
        assert!(
            denied_quiet && ipv6_quiet != Some(false),
            "denied listener received connection"
        );
    }
    pub(super) fn verify_controls(&self) {
        self.outside.verify();
        self.owner.verify();
        eprintln!(
            "phase=two-tunnel-controls outside-read-canary=unchanged owner-write-canary=unchanged"
        );
    }
    fn spawn_control(&self, until: Instant) {
        let profile = self
            .root
            .profile(
                hf_platform::qualification::ProfilePolicy::ControlledHelper,
                &hf_platform::qualification::qualification_read_paths(),
                Some(self.proxy.as_ref().unwrap().port()),
            )
            .unwrap();
        let args = ["-I", "-S", HELPER, "--spawn-control"].map(OsString::from);
        let result = actors::run(
            &self.root,
            &profile,
            Path::new(PYTHON),
            &args,
            environment(&[]),
            until,
            4096,
        );
        actors::require_clean(&result, "controlled-spawn-positive");
        assert_eq!(result.stdout, b"spawn-control=allowed\n");
        assert!(result.stderr.is_empty());
    }
    pub(super) fn cleanup(self) {
        self.require_denied_quiet();
        self.verify_controls();
        let root = self.root.root().to_path_buf();
        self.root
            .validate_topology()
            .expect("settled root topology");
        self.outside.remove();
        fs::remove_dir_all(&root).expect("remove verified owned fixture root");
        eprintln!(
            "phase=cleanup root={} nonce={} owned-root=removed original-read-canary=removed actors=settled",
            root.display(),
            self.nonce
        );
    }
}
