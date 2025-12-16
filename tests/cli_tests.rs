use assert_cmd::Command;
use predicates::prelude::*;

#[test]
#[allow(deprecated)]
fn test_collect_help() {
    Command::cargo_bin("symmetri-collect")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Collect system metrics"));
}

#[test]
#[allow(deprecated)]
fn test_report_help() {
    Command::cargo_bin("symmetri-report")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Render a timeframe report"));
}

#[test]
#[allow(deprecated)]
fn test_main_binary_help() {
    Command::cargo_bin("symmetri")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("System metrics collection"));
}

#[test]
#[allow(deprecated)]
fn test_collect_subcommand() {
    Command::cargo_bin("symmetri")
        .unwrap()
        .arg("collect")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("collect"));
}

#[test]
#[allow(deprecated)]
fn test_report_subcommand() {
    Command::cargo_bin("symmetri")
        .unwrap()
        .arg("report")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("report"));
}
