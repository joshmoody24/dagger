//! Reads the TypeScript fixture the way a person would, and checks what comes back.
//!
//! The only test that exercises a language server, an adapter subprocess, and the reading
//! order together. Everything else in the suite tests one of those apart from the others,
//! and most of the mistakes so far have been in how they meet.

use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn the_typescript_fixture_reads_as_expected() {
    let fixture = fixture();
    let adapter = target().join("dagger-lsp");

    /* A skipped test looks exactly like a passing one, and this is the only test that puts
     * the whole thing together — so it says so rather than going quiet. The dev shell
     * provides the compiler, which means an absence here is a broken setup, not a fact
     * about somebody's machine. Anyone genuinely without one can say so and get the rest of
     * the suite. */
    if !on_path("tsc") || !adapter.exists() {
        let missing = if on_path("tsc") {
            format!("{} hasn't been built — run ./build", adapter.display())
        } else {
            "tsc isn't on PATH — this needs the dev shell, or nix develop".to_string()
        };
        assert!(
            std::env::var_os("DAGGER_WITHOUT_TSC").is_some(),
            "the one test that reads a repository end to end can't run: {missing}.\n\
             Set DAGGER_WITHOUT_TSC=1 to skip it on purpose."
        );
        eprintln!("skipped on purpose: {missing}");
        return;
    }

    let run = Command::new(env!("CARGO_BIN_EXE_dagger"))
        .args(["--list", "before", "after"])
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

/// Where cargo put the binaries, worked out from where it put this test.
fn target() -> PathBuf {
    std::env::current_exe()
        .expect("a test knows where it is")
        .parent()
        .and_then(|deps| deps.parent())
        .expect("target/debug")
        .to_path_buf()
}

fn on_path(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|run| run.status.success())
}
