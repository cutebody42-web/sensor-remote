#![cfg(windows)]
use std::process::Command;

#[test]
fn compiled_cli_preserves_dpapi_identity_across_processes() {
    let directory = tempfile::tempdir().unwrap();
    let identity = || {
        Command::new(env!("CARGO_BIN_EXE_SENSOR-CLI"))
            .arg("identity")
            .arg(directory.path())
            .output()
            .unwrap()
    };
    let first = identity();
    let second = identity();
    assert!(first.status.success());
    assert!(second.status.success());
    assert_eq!(first.stdout, second.stdout);
    let output = String::from_utf8(first.stdout).unwrap();
    assert!(output.contains("Storage: Windows user DPAPI"));
    assert!(output.contains("Network: offline"));
    assert!(directory.path().join("identity.bin").is_file());
}

#[test]
fn invalid_cli_arguments_do_not_initialize_a_profile() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("must-not-be-created");
    let result = Command::new(env!("CARGO_BIN_EXE_SENSOR-CLI"))
        .arg("unknown")
        .arg(&missing)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!missing.exists());
    let version = Command::new(env!("CARGO_BIN_EXE_SENSOR-CLI"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert!(String::from_utf8(version.stdout)
        .unwrap()
        .contains(env!("CARGO_PKG_VERSION")));
}
