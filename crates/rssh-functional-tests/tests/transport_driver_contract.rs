use std::{path::PathBuf, process::Command, time::Duration};

use rssh_functional_tests::{run_ssh_loopback_journey, run_transfer_roundtrip_journey};
use sha2::{Digest, Sha256};

const DEADLINE: Duration = Duration::from_secs(30);

fn app() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/rssh-app");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    assert!(
        path.is_file(),
        "build rssh-app before transport contract: {}",
        path.display()
    );
    path
}

#[test]
fn ssh_journey_runs_native_and_system_application_entrypoints_against_one_loopback_server() {
    if !system_ssh_available() {
        return;
    }
    let result = run_ssh_loopback_journey(&app(), DEADLINE, Duration::from_secs(3)).unwrap();
    assert!(result.native_stdout.contains("functional-ssh-native"));
    assert!(result.system_stdout.contains("functional-ssh-system"));
    assert!(
        result
            .server_trace
            .iter()
            .any(|event| event.contains("Exec"))
    );
    assert!(result.resources_zero);
    assert_eq!(result.local_forward_echo, b"functional-local-forward");
    assert_eq!(result.dynamic_forward_echo, b"functional-dynamic-forward");
    assert_eq!(result.remote_forward_echo, b"functional-remote-forward");
    assert!(
        result
            .server_trace
            .iter()
            .any(|event| event.contains("DirectTcpip"))
    );
    assert!(
        result
            .server_trace
            .iter()
            .any(|event| event.contains("RemoteForward") && event.contains("accepted: true"))
    );
}

#[test]
fn transfer_journey_preserves_sftp_and_scp_bytes_by_sha256() {
    if !system_transfer_tools_available() {
        return;
    }
    let result = run_transfer_roundtrip_journey(&app(), DEADLINE, Duration::from_secs(3)).unwrap();
    let expected = Sha256::digest(b"rssh-functional-transfer-content\0\xff");
    assert_eq!(result.expected_sha256.as_slice(), expected.as_slice());
    assert_eq!(result.sftp_download_sha256, result.expected_sha256);
    assert_eq!(result.scp_download_sha256, result.expected_sha256);
    assert!(result.resources_zero);
}

#[test]
fn forwarding_journey_explicitly_reaps_its_owned_process_before_reporting_cleanup() {
    let source = include_str!("../src/transport_driver.rs");
    assert!(source.contains(".terminate()"));
    assert!(source.contains("terminate native forwarding"));
    assert!(!source.contains("drop(forward_process)"));
}

fn system_ssh_available() -> bool {
    let available = Command::new("ssh").arg("-V").output().is_ok();
    assert!(
        available,
        "required system OpenSSH capability is unavailable"
    );
    available
}

fn system_transfer_tools_available() -> bool {
    let available = ["sftp", "scp"]
        .into_iter()
        .all(|tool| Command::new(tool).arg("-h").output().is_ok());
    assert!(
        available,
        "required system SFTP/SCP capability is unavailable"
    );
    available
}
