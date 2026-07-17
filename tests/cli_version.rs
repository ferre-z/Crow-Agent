use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_subcommand_matches_flag() {
    let flag_output = Command::cargo_bin("crow")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let subcmd_output = Command::cargo_bin("crow")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(flag_output, subcmd_output);
}

#[test]
fn version_subcommand_prints_version_string() {
    Command::cargo_bin("crow")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("crow "));
}

#[test]
fn version_subcommand_exits_zero() {
    Command::cargo_bin("crow")
        .unwrap()
        .arg("version")
        .assert()
        .success();
}
