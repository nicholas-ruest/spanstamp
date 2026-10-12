use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

fn spanstamp() -> Command {
    Command::cargo_bin("spanstamp").unwrap()
}

#[test]
fn index_emits_machine_readable_blocks() {
    let directory = tempdir().unwrap();
    let document = directory.path().join("notes.md");
    fs::write(&document, "# Notes\n\nTarget paragraph.\n").unwrap();

    let output = spanstamp()
        .args(["index", document.to_str().unwrap(), "--compact"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(value["blocks"][1]["start_line"], 3);
}

#[test]
fn capture_and_resolve_across_a_line_shift() {
    let directory = tempdir().unwrap();
    let document = directory.path().join("notes.md");
    let anchor = directory.path().join("anchor.json");
    fs::write(&document, "# Notes\n\nTarget paragraph.\n").unwrap();

    spanstamp()
        .args([
            "capture",
            document.to_str().unwrap(),
            "--line",
            "3",
            "--output",
            anchor.to_str().unwrap(),
        ])
        .assert()
        .success();

    fs::write(
        &document,
        "Inserted context.\n\n# Notes\n\nTarget paragraph.\n",
    )
    .unwrap();

    spanstamp()
        .args([
            "resolve",
            document.to_str().unwrap(),
            anchor.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("RESOLVED"))
        .stdout(predicate::str::contains("lines 5-5"));
}

#[test]
fn unresolved_anchor_uses_exit_code_two_and_json_status() {
    let directory = tempdir().unwrap();
    let document = directory.path().join("notes.md");
    let anchor = directory.path().join("anchor.json");
    fs::write(&document, "Original claim.\n").unwrap();

    spanstamp()
        .args([
            "capture",
            document.to_str().unwrap(),
            "--line",
            "1",
            "--output",
            anchor.to_str().unwrap(),
        ])
        .assert()
        .success();
    fs::write(&document, "Changed claim.\n").unwrap();

    spanstamp()
        .args([
            "resolve",
            document.to_str().unwrap(),
            anchor.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("\"status\": \"missing\""));
}

#[test]
fn malformed_anchor_fails_closed() {
    let directory = tempdir().unwrap();
    let document = directory.path().join("notes.md");
    let anchor = directory.path().join("anchor.json");
    fs::write(&document, "Claim.\n").unwrap();
    fs::write(&anchor, "{\"schema_version\":1}").unwrap();

    spanstamp()
        .args([
            "resolve",
            document.to_str().unwrap(),
            anchor.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("failed to parse"));
}
