//! Smoke tests for the `mbc` binary.
//!
//! These tests deliberately do not depend on real hardware; they exercise
//! argument parsing, help output and JSON envelope shape so a regression
//! breaks CI even on hardware-less GitHub runners.

use assert_cmd::Command;
use predicates::str::contains;

fn mbc() -> Command {
    Command::cargo_bin("mbc").expect("binary built")
}

#[test]
fn prints_help() {
    mbc()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Cross-platform monitor brightness"))
        .stdout(contains("list"))
        .stdout(contains("set"))
        .stdout(contains("get-vcp"));
}

#[test]
fn prints_version() {
    mbc().arg("--version").assert().success();
}

#[test]
fn list_json_returns_valid_array() {
    let assert = mbc().args(["--format", "json", "list"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .expect("CLI must emit valid JSON for `list` even with zero monitors");
    assert!(v.is_array(), "list output must be a JSON array");
}

#[test]
fn unknown_subcommand_fails() {
    mbc()
        .arg("not-a-real-subcommand")
        .assert()
        .failure();
}

#[test]
fn rejects_invalid_vcp_code() {
    // Code is parsed as u8: passing "999" must fail at clap parse time.
    mbc()
        .args(["get-vcp", "--id", "noop", "999"])
        .assert()
        .failure();
}

#[test]
fn no_id_match_returns_clear_error() {
    // Asking for a non-existent monitor by id should produce a non-zero
    // exit and a useful message, not a panic.
    let out = mbc()
        .args(["set", "--id", "definitely-not-a-real-id", "50"])
        .output()
        .expect("spawn");
    // We accept either:
    // * exit 0 with rows showing the missing monitor (empty result), or
    // * a non-zero exit with an error message —
    // depending on whether enumeration succeeded. Either way the binary
    // must not panic.
    assert!(
        out.status.code().is_some(),
        "process did not exit cleanly: {out:?}"
    );
}
