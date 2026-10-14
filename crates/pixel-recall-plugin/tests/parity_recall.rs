// SPDX-FileCopyrightText: The Pixel contributors
// SPDX-License-Identifier: MIT

//! `pixel-recall` against the built-in `pixel recall`, on a scratch corpus:
//! one Claude transcript under a throwaway `HOME`, indexed by the binary under
//! test, then read back through every verb whose output an agent parses.
//!
//! The cases are those of core's `crates/pixel/tests/cli/recall_cli.rs`, with
//! goldens recorded from the built-in. No case needs the embedding model:
//! `ask` and `context` run `--lexical-only`.

use pixel_recall_plugin::testing::{Setup, assert_parity, scratch_dir, write};

const BIN: &str = env!("CARGO_BIN_EXE_pixel-recall");

const SESSION_ID: &str = "0123abcd-0000-4000-8000-000000000001";
/// The human turn: the needle, then enough text that its full-turn block
/// cannot fit a 100-token context budget while the header line can.
const NEEDLE: &str = "please fix the streamed needle";
const TAIL: &str = " it fails on the second retry and the log shows nothing useful";

fn parity(case: &str, make: &dyn Fn() -> Setup) {
    assert_parity(env!("CARGO_MANIFEST_DIR"), BIN, "recall", case, make);
}

/// A scratch `HOME` holding one two-turn Claude session (a human prompt with
/// the needle and a harness-injected reminder), `PIXEL_RECALL_DIR` pointing
/// into it, and `index` already run when `indexed` is set.
fn corpus(args: &[&str], human_turn: &str, indexed: bool) -> Setup {
    let mut setup = Setup::new(scratch_dir(), args);
    let home = setup.scratch();
    let lines = [
        serde_json::json!({
            "type": "user", "cwd": "/work/pixel", "gitBranch": "develop",
            "timestamp": "2025-10-09T08:53:20.000Z",
            "message": {"content": [{"type": "text", "text": human_turn}]},
        })
        .to_string(),
        serde_json::json!({
            "type": "user", "cwd": "/work/pixel",
            "timestamp": "2025-10-09T08:54:20.000Z",
            "message": {"content": [{"type": "text", "text": "<system-reminder>injected context</system-reminder>"}]},
        })
        .to_string(),
    ];
    write(
        &home,
        &format!(".claude/projects/-work-pixel/{SESSION_ID}.jsonl"),
        &format!("{}\n", lines.join("\n")),
    );
    let setup = setup
        .env("HOME", &home)
        .env("PIXEL_RECALL_DIR", home.join("recall"));
    if indexed {
        setup.then(&["index", "--source", "claude"])
    } else {
        setup
    }
}

fn human_turn() -> String {
    format!("{NEEDLE}{}", TAIL.repeat(8))
}

/// An indexed corpus and `args` to run on it.
fn indexed(args: &[&str]) -> Setup {
    corpus(args, &human_turn(), true)
}

/// A corpus nobody has indexed: the query catches up on demand.
fn cold(args: &[&str]) -> Setup {
    corpus(args, &human_turn(), false)
}

// --- index -----------------------------------------------------------------

#[test]
fn index_reports_what_it_ingested() {
    parity("index-stats", &|| {
        cold(&["index", "--source", "claude", "--stats"])
    });
}

#[test]
fn index_rebuilds_the_lexical_segments_on_full() {
    parity("index-full", &|| {
        cold(&["index", "--source", "claude", "--full"])
    });
}

// --- search ----------------------------------------------------------------

#[test]
fn search_returns_the_matching_turn_as_json() {
    parity("search-json", &|| {
        indexed(&["search", "streamed needle", "--json"])
    });
}

#[test]
fn search_names_the_session_in_its_text_lines() {
    parity("search-text", &|| indexed(&["search", "streamed needle"]));
}

#[test]
fn search_reports_a_miss() {
    parity("search-miss", &|| {
        indexed(&["search", "no-such-token-xyzzy"])
    });
}

#[test]
fn search_on_a_cold_corpus_ingests_on_demand() {
    parity("search-cold-json", &|| {
        cold(&["search", "streamed needle", "--json"])
    });
}

#[test]
fn search_validates_and_applies_the_role_filter() {
    parity("search-role-bogus", &|| {
        indexed(&["search", "needle", "--role", "bogus"])
    });
    parity("search-role-user-json", &|| {
        indexed(&["search", "needle", "--role", "user", "--json"])
    });
    parity("search-role-tool-json", &|| {
        indexed(&["search", "needle", "--role", "tool", "--json"])
    });
}

#[test]
fn search_can_drop_harness_turns_and_match_words() {
    parity("search-human-only-word-json", &|| {
        indexed(&["search", "reminder", "--human-only", "--word", "--json"])
    });
}

// --- sessions, show --------------------------------------------------------

#[test]
fn sessions_lists_the_session_as_json_and_text() {
    parity("sessions-json", &|| indexed(&["sessions", "--json"]));
    parity("sessions-text", &|| indexed(&["sessions"]));
    parity("sessions-cold-json", &|| cold(&["sessions", "--json"]));
}

#[test]
fn show_prints_the_turns_as_json_and_marks_harness_ones_in_text() {
    parity("show-json", &|| {
        indexed(&["show", "claude:0123abcd", "--json"])
    });
    parity("show-text", &|| indexed(&["show", "claude:0123abcd"]));
    parity("show-cold-text", &|| cold(&["show", "claude:0123abcd"]));
}

#[test]
fn show_takes_a_turn_range() {
    parity("show-turn-range", &|| {
        indexed(&["show", "claude:0123abcd", "--turn", "1..1"])
    });
    parity("show-turn-out-of-range", &|| {
        indexed(&["show", "claude:0123abcd", "--turn", "5..9"])
    });
    parity("show-unknown-session", &|| {
        indexed(&["show", "claude:ffff"])
    });
}

/// `show --json` travels under the global stdout cap: capped it is still one
/// JSON object marked `truncated`, uncapped it is complete.
#[test]
fn show_json_is_capped_on_the_stdout_cap() {
    let turn = format!("{NEEDLE} {}", "retry noise ".repeat(1400));
    parity("show-json-capped", &|| {
        corpus(&["show", "claude:0123abcd", "--json"], &turn, true)
            .env("PIXEL_OUTPUT_CAP_BYTES", "4096")
    });
    parity("show-json-uncapped", &|| {
        corpus(&["show", "claude:0123abcd", "--json"], &turn, true)
            .env("PIXEL_OUTPUT_CAP_BYTES", "0")
    });
}

// --- ask, context ----------------------------------------------------------

#[test]
fn ask_groups_the_matching_session_from_the_lexical_channel() {
    parity("ask-lexical-json", &|| {
        indexed(&["ask", "streamed needle", "--lexical-only", "--json"])
    });
    parity("ask-lexical-text", &|| {
        indexed(&["ask", "streamed needle", "--lexical-only"])
    });
    parity("ask-lexical-miss", &|| {
        indexed(&["ask", "no-such-token-xyzzy", "--lexical-only"])
    });
    parity("ask-k-out-of-range", &|| {
        indexed(&["ask", "needle", "--lexical-only", "--k", "0"])
    });
}

#[test]
fn context_packs_the_matching_session_within_the_budget() {
    parity("context-500", &|| {
        indexed(&[
            "context",
            "streamed needle",
            "--lexical-only",
            "--budget",
            "500",
        ])
    });
    parity("context-100-drops-the-turn", &|| {
        indexed(&[
            "context",
            "streamed needle",
            "--lexical-only",
            "--budget",
            "100",
        ])
    });
    parity("context-budget-too-small", &|| {
        indexed(&["context", "x", "--budget", "10"])
    });
    parity("context-wrong-agent", &|| {
        indexed(&[
            "context",
            "streamed needle",
            "--lexical-only",
            "--agent",
            "codex",
        ])
    });
}

// --- status, export, daemon ------------------------------------------------

#[test]
fn status_reports_the_corpus_counts_and_location() {
    parity("status-json", &|| indexed(&["status", "--json"]));
    parity("status-text", &|| indexed(&["status"]));
}

#[test]
fn export_writes_one_file_per_session() {
    parity("export-jsonl", &|| {
        let mut setup = indexed(&[]);
        let out = setup.scratch().join("export");
        setup.args = [
            "export",
            "--out",
            &out.display().to_string(),
            "--format",
            "jsonl",
        ]
        .map(String::from)
        .to_vec();
        setup
    });
}

#[test]
fn the_daemon_probe_reports_a_missing_daemon_and_opens_no_repository_service() {
    parity("daemon-status", &|| cold(&["daemon", "status"]));
    parity("daemon-stop", &|| cold(&["daemon", "stop"]));
}
