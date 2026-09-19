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

    // The fixture is about this adapter, and a missing TypeScript compiler says nothing
    // about whether dagger works. Skipping beats failing on someone else's machine.
    if !on_path("tsc") || !adapter.exists() {
        eprintln!("skipped: needs tsc on PATH and a built dagger-lsp");
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
