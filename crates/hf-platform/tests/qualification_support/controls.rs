use super::*;

pub fn vendor_no_fork_profile_denies_both_fork_apis() {
    let mut owned = SettledRoot::create();
    for operation in ["fork", "posix-spawn"] {
        let (code, result) = launch_with_policy(
            &owned.fixture,
            ProfilePolicy::VendorNoFork,
            operation,
            Path::new("unused"),
        );
        assert_eq!(code, 0, "{operation} probe must report an observation");
        assert_eq!(
            result.trim(),
            "RESULT denied errno=1",
            "VendorNoFork must deny {operation}"
        );
    }
    owned.settle();
}

pub fn controlled_route(operation: &str) {
    let mut owned = SettledRoot::create();
    if operation == "nested-sandbox" {
        nested_sandbox_positive_baseline();
    }
    let witness = owned.fixture.outside().join("route-witness");
    fs::write(&witness, b"route-original").unwrap();
    let (code, result) = launch_with_policy(
        &owned.fixture,
        ProfilePolicy::ControlledHelper,
        operation,
        &witness,
    );
    assert_eq!(code, 0, "{operation} route must report its observation");
    let allowed_result = operation == "nested-sandbox"
        && result.trim() == "RESULT unsupported nested-apply-refused exit=71";
    assert!(
        allowed_result || result.trim() == "RESULT denied errno=1",
        "{operation} route must deny the outside write or report an evidenced nested apply refusal: {result:?}"
    );
    assert_eq!(
        fs::read(&witness).unwrap(),
        b"route-original",
        "{operation} descendant must not modify the denied outside witness"
    );
    owned.settle();
}

fn nested_sandbox_positive_baseline() {
    let child = Command::new("/usr/bin/sandbox-exec")
        .args(["-p", "(version 1) (allow default)", "/usr/bin/true"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("known nested profile syntax baseline starts");
    let mut child = DirectChild::new(child);
    let output = child.wait_output(5000);
    assert!(
        output.status.success(),
        "same nested allow-default syntax must work outside the outer profile: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}
