//! Run only on a host with the Winbridge libvirt VM configured:
//! `cargo test --features integration --test vm_integration`

#![cfg(feature = "integration")]

#[test]
fn status_command_against_real_libvirt_returns_known_state() {
    let bin = env!("CARGO_BIN_EXE_winbridge");
    let output = std::process::Command::new(bin)
        .arg("status")
        .output()
        .expect("failed to invoke winbridge");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected_states = ["Active", "Saved", "Off", "Other"];
    assert!(
        expected_states.iter().any(|state| stdout.contains(state)),
        "stdout: {stdout}"
    );
}
