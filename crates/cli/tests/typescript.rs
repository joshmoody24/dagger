//! The only test that runs a language server, an adapter subprocess, and the reading
//! order together. Most mistakes so far have been where they meet.

use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn the_typescript_fixture_reads_as_expected() {
    let fixture = fixture();

    // Fails rather than skipping quietly: a skipped test looks like a passing one, and the
    // dev shell provides tsc, so its absence is a broken setup. DAGGER_WITHOUT_TSC opts out.
    if !on_path("tsc") {
        let missing = "tsc isn't on PATH — this needs the dev shell, or nix develop";
        assert!(
            std::env::var_os("DAGGER_WITHOUT_TSC").is_some(),
            "the one test that reads a repository end to end can't run: {missing}.\n\
             Set DAGGER_WITHOUT_TSC=1 to skip it on purpose."
        );
        eprintln!("skipped on purpose: {missing}");
        return;
    }

    let run = Command::new(env!("CARGO_BIN_EXE_dagger"))
        .args(["cli", "--list", "before", "after"])
        .current_dir(&fixture)
        .output()
        .expect("dagger should run");

    let read = String::from_utf8_lossy(&run.stdout);
    let expected = std::fs::read_to_string(fixture.join("expected.txt")).expect("expected.txt");

    assert_eq!(
        read.trim(),
        expected.trim(),
        "the reading changed.\n\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/typescript")
        .canonicalize()
        .expect("the fixture should be where it always is")
}

fn on_path(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|run| run.status.success())
}
