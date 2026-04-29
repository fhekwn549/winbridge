#[test]
fn winbridge_help_exits_zero_with_subcommands_listed() {
    let bin = env!("CARGO_BIN_EXE_winbridge");
    let output = std::process::Command::new(bin)
        .arg("--help")
        .output()
        .expect("failed to invoke winbridge");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("start"));
    assert!(stdout.contains("stop"));
    assert!(stdout.contains("status"));
}

#[test]
fn winbridge_status_unknown_arg_returns_nonzero() {
    let bin = env!("CARGO_BIN_EXE_winbridge");
    let output = std::process::Command::new(bin)
        .args(["status", "--bogus"])
        .output()
        .expect("failed to invoke winbridge");

    assert!(!output.status.success());
}
