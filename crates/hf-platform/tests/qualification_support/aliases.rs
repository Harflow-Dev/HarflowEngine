use super::*;

pub fn hardened_ancestor_rename_is_denied() {
    let mut owned = SettledRoot::create();
    let root = &owned.fixture;
    let protected = root.protected();
    root.validate_topology().unwrap();
    let ancestor = protected.parent().unwrap().to_path_buf();
    let witness = protected.join("bytes");
    fs::write(&witness, b"hardened-ancestor").unwrap();
    let before = fs::metadata(&witness).unwrap();
    let (code, result) = launch_with_policy(
        root,
        ProfilePolicy::ControlledHelper,
        "ancestor-escape",
        &ancestor,
    );
    assert_eq!(code, 0);
    assert_eq!(result.trim(), "RESULT denied errno=1");
    let after = fs::metadata(&witness).unwrap();
    assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
    assert_eq!(fs::read(&witness).unwrap(), b"hardened-ancestor");
    assert!(!ancestor.with_extension("moved").exists());
    root.validate_topology().unwrap();
    owned.settle();
}

pub fn new_hardlink_syscalls_are_denied() {
    let mut owned = SettledRoot::create();
    let root = &owned.fixture;
    root.validate_topology().unwrap();
    let workspace_source = root.workspace().join("link-source");
    let workspace_destination = root.workspace().join("nested/control/new-link");
    fs::write(&workspace_source, b"workspace-link-source").unwrap();
    assert_link_control(&workspace_source, &workspace_destination);
    assert_link_denied(
        root,
        "hardlink-control",
        &workspace_source,
        &workspace_destination,
    );

    let outside_source = root.outside().join("link-source");
    let allowed_destination = root.workspace().join("new-link");
    fs::write(&outside_source, b"outside-link-source").unwrap();
    assert_link_control(&outside_source, &allowed_destination);
    assert_link_denied(
        root,
        "hardlink-workspace",
        &outside_source,
        &allowed_destination,
    );
    root.validate_topology().unwrap();
    owned.settle();
}

fn assert_link_control(source: &Path, destination: &Path) {
    let before = fs::metadata(source).unwrap();
    fs::hard_link(source, destination).expect("unconfined hardlink syscall positive control");
    let source_identity = fs::metadata(source).unwrap();
    let destination_identity = fs::metadata(destination).unwrap();
    assert_eq!(
        (source_identity.dev(), source_identity.ino()),
        (destination_identity.dev(), destination_identity.ino())
    );
    assert_eq!(destination_identity.nlink(), before.nlink() + 1);
    fs::remove_file(destination).unwrap();
    assert_eq!(fs::metadata(source).unwrap().nlink(), before.nlink());
}

fn assert_link_denied(root: &FixtureRoot, operation: &str, source: &Path, destination: &Path) {
    assert!(!destination.exists());
    let source_before = fs::metadata(source).unwrap();
    let bytes_before = fs::read(source).unwrap();
    let (code, result) =
        launch_with_policy(root, ProfilePolicy::ControlledHelper, operation, source);
    assert_eq!(code, 0, "{operation} syscall reports an outcome");
    assert_eq!(result.trim(), "RESULT denied errno=1");
    assert!(!destination.exists());
    let source_after = fs::metadata(source).unwrap();
    assert_eq!(
        (source_after.dev(), source_after.ino(), source_after.nlink()),
        (
            source_before.dev(),
            source_before.ino(),
            source_before.nlink()
        )
    );
    assert_eq!(fs::read(source).unwrap(), bytes_before);
}
