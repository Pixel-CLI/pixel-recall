// SPDX-FileCopyrightText: The Pixel contributors
// SPDX-License-Identifier: MIT

//! `pixel-recall` as a process: how it takes its arguments and how it talks to
//! the recall daemon. The cases are the daemon ones of core's
//! `crates/pixel/tests/cli/recall_cli.rs` (a fake daemon on the socket, the
//! probe that must never open a repository service) plus a real daemon
//! started, asked and stopped through the binary.

use std::path::Path;

use pixel_recall_plugin::testing::{Seen, fake_recall_daemon, run, scratch_dir, write};
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_pixel-recall");
const SESSION_ID: &str = "0123abcd-0000-4000-8000-000000000001";

/// A scratch `HOME` with one Claude session mentioning the needle.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Self {
        let dir = scratch_dir();
        let line = serde_json::json!({
            "type": "user", "cwd": "/work/pixel", "gitBranch": "develop",
            "timestamp": "2025-10-09T08:53:20.000Z",
            "message": {"content": [{"type": "text", "text": "please fix the streamed needle"}]},
        });
        write(
            dir.path(),
            &format!(".claude/projects/-work-pixel/{SESSION_ID}.jsonl"),
            &format!("{line}\n"),
        );
        std::fs::create_dir_all(dir.path().join("recall")).unwrap();
        Self { dir }
    }

    fn recall_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("recall").canonicalize().unwrap()
    }

    fn run(&self, args: &[&str]) -> Seen {
        let home = self.dir.path().display().to_string();
        let recall = self.recall_dir().display().to_string();
        run(
            BIN.as_ref(),
            args,
            self.dir.path(),
            &[("HOME", &home), ("PIXEL_RECALL_DIR", &recall)],
        )
    }
}

#[test]
fn the_version_is_the_package_version() {
    let seen = run(BIN.as_ref(), &["--version"], Path::new("."), &[]);
    assert_eq!(
        seen.stdout,
        format!("pixel-recall {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn naming_the_command_in_front_changes_nothing() {
    let home = Home::new();
    // The first run creates the database; compare two runs on the same file.
    assert_eq!(home.run(&["status", "--json"]).code, 0);
    let plain = home.run(&["status", "--json"]);
    let named = home.run(&["recall", "status", "--json"]);
    assert_eq!(plain.code, 0, "{}", plain.stderr);
    assert_eq!(plain, named);
}

#[test]
fn no_subcommand_is_a_usage_error() {
    let seen = run(BIN.as_ref(), &[], Path::new("."), &[]);
    assert_eq!(seen.code, 2, "{}", seen.stderr);
    assert!(
        seen.stderr.contains("Usage: pixel recall"),
        "{}",
        seen.stderr
    );
}

/// The daemon probe must never open a repository `Service` on the corpus
/// directory: `Service::open` leaves `.pixel/base.shard` in the corpus, and a
/// repository daemon answers `Ping` but refuses `Recall`.
#[test]
fn daemon_status_never_starts_a_repo_service_on_the_corpus() {
    let home = Home::new();
    let seen = home.run(&["daemon", "status"]);
    assert_eq!(seen.code, 0, "{}", seen.stderr);
    assert!(
        seen.stdout.contains("daemon not running"),
        "{}",
        seen.stdout
    );
    assert!(
        !home.recall_dir().join(".pixel/base.shard").exists(),
        "a repo Service was opened on the corpus"
    );
}

/// A recall daemon that answers an op with an error is named on stderr and
/// the answer still comes from the corpus in-process: a daemon with a corrupt
/// vector store used to be indistinguishable from no daemon at all.
#[test]
fn search_names_a_failing_recall_daemon_then_answers_in_process() {
    let home = Home::new();
    assert_eq!(home.run(&["index", "--source", "claude"]).code, 0);
    let server = fake_recall_daemon(
        &home.recall_dir(),
        pixel_daemon::api::failure_response("recall", "vector store is corrupt"),
    );
    let seen = home.run(&["search", "streamed needle", "--json"]);
    assert_eq!(seen.code, 0, "{}", seen.stderr);
    assert!(
        seen.stderr.contains("vector store is corrupt"),
        "{}",
        seen.stderr
    );
    let value: Value = serde_json::from_str(&seen.stdout).expect("hits from the in-process path");
    assert_eq!(value["hits"].as_array().map(Vec::len), Some(1), "{value}");
    server.join().unwrap();
    let _ = std::fs::remove_file(pixel_daemon::daemon::socket_path(&home.recall_dir()));
}

/// A daemon that answers is the one that serves the op: its text is printed
/// as is, and so is its JSON under `--json`.
#[test]
fn search_prints_what_the_recall_daemon_answers() {
    let home = Home::new();
    assert_eq!(home.run(&["index", "--source", "claude"]).code, 0);
    let answer = || {
        pixel_daemon::Response::success(
            "recall",
            serde_json::json!({"text": "from daemon\n", "json": {"hits": []}}),
        )
    };
    let server = fake_recall_daemon(&home.recall_dir(), answer());
    let seen = home.run(&["search", "streamed needle"]);
    server.join().unwrap();
    assert_eq!(seen.stdout, "from daemon\n", "{}", seen.stderr);
    let _ = std::fs::remove_file(pixel_daemon::daemon::socket_path(&home.recall_dir()));
    let server = fake_recall_daemon(&home.recall_dir(), answer());
    let seen = home.run(&["search", "streamed needle", "--json"]);
    server.join().unwrap();
    assert_eq!(seen.stdout, "{\"hits\":[]}\n", "{}", seen.stderr);
    let _ = std::fs::remove_file(pixel_daemon::daemon::socket_path(&home.recall_dir()));
}

/// A real corpus daemon, started through the binary (which re-runs itself as
/// `pixel-recall daemon start --foreground`), asked, and stopped.
#[test]
fn the_corpus_daemon_starts_answers_and_stops() {
    let home = Home::new();
    let started = home.run(&["daemon", "start"]);
    // Stop it whatever the assertions say.
    let stop = || home.run(&["daemon", "stop"]);
    let outcome = std::panic::catch_unwind(|| {
        assert_eq!(started.code, 0, "{}", started.stderr);
        assert!(
            started.stdout.starts_with("recall daemon started"),
            "{}",
            started.stdout
        );
        assert!(
            home.run(&["daemon", "status"])
                .stdout
                .starts_with("daemon running")
        );
        let again = home.run(&["daemon", "start"]);
        assert!(
            again.stdout.starts_with("recall daemon already running"),
            "{}",
            again.stdout
        );
        let found = home.run(&["search", "streamed needle", "--json"]);
        assert_eq!(found.code, 0, "{}", found.stderr);
        let value: Value = serde_json::from_str(&found.stdout).unwrap();
        assert_eq!(value["hits"].as_array().map(Vec::len), Some(1), "{value}");
    });
    let stopped = stop();
    assert!(outcome.is_ok(), "the daemon scenario failed");
    assert_eq!(stopped.stdout, "daemon stopped\n");
    assert!(
        home.run(&["daemon", "status"])
            .stdout
            .starts_with("daemon not running")
    );
}
