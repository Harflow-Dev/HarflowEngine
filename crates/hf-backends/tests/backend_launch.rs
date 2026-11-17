#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Normal Claude invocation preparation: bounded inputs, measured executable identity,
//! and a live stdin protocol with the user prompt kept out of argv.
use hf_application::task_launch::{ClaudeDiscoveryPort, LiveInputCodecPort};
use hf_backends::backend_launch::{CLAUDE_VERSION, MAX_PROMPT_BYTES, prepare_claude};
use hf_domain::{
    capture::FileIdentity,
    ownership::Digest,
    prepared_launch::{BinaryMetadata, BinaryPin},
    task_launch::{
        CliSessionId, DiscoveredClaude, LaunchPermission, LaunchRefusal, LaunchSpec, PreparedStdin,
    },
};
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

struct Discovery {
    calls: Cell<usize>,
    result: Result<DiscoveredClaude, LaunchRefusal>,
}
impl ClaudeDiscoveryPort for Discovery {
    fn discover(&mut self, _: Instant) -> Result<DiscoveredClaude, LaunchRefusal> {
        self.calls.set(self.calls.get() + 1);
        self.result.clone()
    }
}

struct Codec {
    calls: Cell<usize>,
    result: Result<Vec<u8>, LaunchRefusal>,
}
impl LiveInputCodecPort for Codec {
    fn encode_user_line(&self, _: &str) -> Result<Vec<u8>, LaunchRefusal> {
        self.calls.set(self.calls.get() + 1);
        self.result.clone()
    }
}

fn spec() -> LaunchSpec {
    LaunchSpec {
        session_id: CliSessionId::new("123e4567-e89b-12d3-a456-426614174000".into()).unwrap(),
        prompt: "Inspect the current source tree.".into(),
        cwd: "/workspace/project".into(),
        permission: LaunchPermission::Unrestricted,
        options: Vec::new(),
    }
}

fn discovered(version: &str) -> DiscoveredClaude {
    DiscoveredClaude {
        binary: BinaryPin {
            logical_path: "/opt/homebrew/bin/claude".into(),
            physical_path: "/opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/cli.js".into(),
            metadata: BinaryMetadata {
                identity: FileIdentity {
                    device: 1,
                    inode: 2,
                },
                length: 4096,
                modified_seconds: 1_700_000_000,
                modified_nanoseconds: 0,
                changed_seconds: 1_700_000_000,
                changed_nanoseconds: 0,
            },
            digest: Digest::new("a".repeat(64)).unwrap(),
        },
        version: version.into(),
    }
}

fn ports(codec_bytes: Vec<u8>) -> (Discovery, Codec) {
    (
        Discovery {
            calls: Cell::new(0),
            result: Ok(discovered(CLAUDE_VERSION)),
        },
        Codec {
            calls: Cell::new(0),
            result: Ok(codec_bytes),
        },
    )
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

#[test]
fn invocation_uses_the_measured_binary_and_fixed_live_protocol_without_prompt_argv() {
    assert_eq!(CLAUDE_VERSION, "2.1.322");
    let spec = spec();
    let prompt = spec.prompt.clone();
    let mut codec_bytes = prompt.as_bytes().to_vec();
    codec_bytes.push(b'\n');
    let (mut discovery, codec) = ports(codec_bytes.clone());
    let prepared = prepare_claude(&spec, &mut discovery, &codec, deadline()).unwrap();
    assert_eq!(prepared.executable.version, CLAUDE_VERSION);
    assert_eq!(prepared.session_id, spec.session_id);
    assert_eq!(
        prepared.session_id.as_str(),
        "123e4567-e89b-12d3-a456-426614174000"
    );
    assert_eq!(
        prepared.executable.binary.logical_path,
        "/opt/homebrew/bin/claude"
    );
    assert_eq!(prepared.cwd, "/workspace/project");
    assert_eq!(prepared.initial_input, codec_bytes);
    assert_eq!(prepared.stdin, PreparedStdin::LivePipe);
    assert_eq!(
        prepared.args.iter().map(String::as_str).collect::<Vec<_>>(),
        vec![
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--permission-mode",
            "bypassPermissions",
            "--input-format",
            "stream-json",
            "--session-id",
            "123e4567-e89b-12d3-a456-426614174000",
        ]
    );
    assert!(!prepared.args.iter().any(|arg| arg.contains(&prompt)));
    assert_eq!((codec.calls.get(), discovery.calls.get()), (1, 1));
}

#[test]
fn unsupported_permission_options_prompt_and_directory_refuse_before_external_ports() {
    let (mut discovery, codec) = ports(b"prompt\n".to_vec());
    let cases = [
        (
            LaunchSpec {
                permission: LaunchPermission::Unsupported("default".into()),
                ..spec()
            },
            LaunchRefusal::UnsupportedPermission,
        ),
        (
            LaunchSpec {
                options: vec!["--dangerously-skip-permissions".into()],
                ..spec()
            },
            LaunchRefusal::UnsupportedOption,
        ),
        (
            LaunchSpec {
                prompt: "  \n".into(),
                ..spec()
            },
            LaunchRefusal::InvalidPrompt,
        ),
        (
            LaunchSpec {
                prompt: "x".repeat(MAX_PROMPT_BYTES + 1),
                ..spec()
            },
            LaunchRefusal::InvalidPrompt,
        ),
        (
            LaunchSpec {
                cwd: "/workspace/../outside".into(),
                ..spec()
            },
            LaunchRefusal::InvalidDirectory,
        ),
    ];
    for (invalid, refusal) in cases {
        assert_eq!(
            prepare_claude(&invalid, &mut discovery, &codec, deadline()),
            Err(refusal)
        );
    }
    assert_eq!((codec.calls.get(), discovery.calls.get()), (0, 0));
}

#[test]
fn cli_session_constructor_rejects_noncanonical_ids_before_backend_ports() {
    let (discovery, codec) = ports(b"prompt\n".to_vec());
    for invalid in [
        "",
        "123e4567-e89b-12d3-a456-42661417400",
        "123e4567-e89b-12d3-a456-4266141740000",
        "123e4567e89b-12d3-a456-426614174000",
        "123E4567-e89b-12d3-a456-426614174000",
        "123e4567-e89b-12d3-a456-42661417400g",
    ] {
        assert_eq!(
            CliSessionId::new(invalid.into()),
            Err(LaunchRefusal::InvalidSession),
            "{invalid}"
        );
    }
    assert_eq!((codec.calls.get(), discovery.calls.get()), (0, 0));
}

#[test]
fn invalid_codec_framing_refuses_before_binary_discovery() {
    let (mut discovery, codec) = ports(b"first\nsecond\n".to_vec());
    assert_eq!(
        prepare_claude(&spec(), &mut discovery, &codec, deadline()),
        Err(LaunchRefusal::Codec)
    );
    assert_eq!((codec.calls.get(), discovery.calls.get()), (1, 0));
}

#[test]
fn unsupported_version_and_unpinned_binary_are_refused_after_discovery() {
    let (mut discovery, codec) = ports(b"prompt\n".to_vec());
    discovery.result = Ok(discovered("2.1.999"));
    assert_eq!(
        prepare_claude(&spec(), &mut discovery, &codec, deadline()),
        Err(LaunchRefusal::UnsupportedVersion)
    );
    assert_eq!((codec.calls.get(), discovery.calls.get()), (1, 1));

    let mut bad = discovered(CLAUDE_VERSION);
    bad.binary.metadata.identity.inode = 0;
    discovery.result = Ok(bad);
    assert_eq!(
        prepare_claude(&spec(), &mut discovery, &codec, deadline()),
        Err(LaunchRefusal::InvalidDiscovery)
    );
    assert_eq!((codec.calls.get(), discovery.calls.get()), (2, 2));
}

#[test]
fn expired_deadline_refuses_before_codec_or_discovery() {
    let (mut discovery, codec) = ports(b"prompt\n".to_vec());
    assert_eq!(
        prepare_claude(&spec(), &mut discovery, &codec, Instant::now()),
        Err(LaunchRefusal::Deadline)
    );
    assert_eq!((codec.calls.get(), discovery.calls.get()), (0, 0));
}
