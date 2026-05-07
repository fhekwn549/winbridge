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
    assert!(stdout.contains("install-desktop-entry"));
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

#[test]
fn winbridge_start_help_lists_window_modes() {
    let bin = env!("CARGO_BIN_EXE_winbridge");
    let output = std::process::Command::new(bin)
        .args(["start", "--help"])
        .output()
        .expect("failed to invoke winbridge");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--mode"));
    assert!(stdout.contains("app"));
    assert!(stdout.contains("desktop"));
    assert!(stdout.contains("--display"));
    assert!(stdout.contains("stable-slots"));
    assert!(stdout.contains("experimental-multimon"));
}

#[test]
fn winbridge_start_accepts_known_window_modes() {
    let bin = env!("CARGO_BIN_EXE_winbridge");

    for mode in ["app", "desktop"] {
        let output = std::process::Command::new(bin)
            .args(["start", "--mode", mode, "--help"])
            .output()
            .expect("failed to invoke winbridge");

        assert!(
            output.status.success(),
            "mode {mode} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn winbridge_start_accepts_known_display_strategies() {
    let bin = env!("CARGO_BIN_EXE_winbridge");

    for display in ["stable-slots", "experimental-multimon"] {
        let output = std::process::Command::new(bin)
            .args(["start", "--display", display, "--help"])
            .output()
            .expect("failed to invoke winbridge");

        assert!(
            output.status.success(),
            "display {display} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn winbridge_start_rejects_unknown_window_mode() {
    let bin = env!("CARGO_BIN_EXE_winbridge");
    let output = std::process::Command::new(bin)
        .args(["start", "--mode", "bogus"])
        .output()
        .expect("failed to invoke winbridge");

    assert!(!output.status.success());
}

#[test]
fn winbridge_start_rejects_unknown_display_strategy() {
    let bin = env!("CARGO_BIN_EXE_winbridge");
    let output = std::process::Command::new(bin)
        .args(["start", "--display", "bogus"])
        .output()
        .expect("failed to invoke winbridge");

    assert!(!output.status.success());
}
