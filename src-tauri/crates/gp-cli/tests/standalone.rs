//! End-to-end process and pseudo-terminal tests, using only Python's stdlib.
#[test]
fn standalone_cli_flows() {
    let status = std::process::Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/standalone.py"))
        .arg(env!("CARGO_BIN_EXE_gp-client"))
        .status()
        .expect("python3 is required for the simulated-helper/PTY tests");
    assert!(status.success());
}
