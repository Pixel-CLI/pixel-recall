// SPDX-FileCopyrightText: The Pixel contributors
// SPDX-License-Identifier: MIT

//! Test support for the command tests (feature `testing`).
//!
//! Each command's tests run `pixel-recall` in a temporary directory and check
//! it two ways:
//!
//! - against a **golden** recorded from the built-in command of Pixel core,
//!   so CI needs no core build;
//! - against the **live** built-in when `PIXEL_CORE_BIN` names a core `pixel`
//!   binary that still has the command (see CONTRIBUTING.md), which is how
//!   the goldens are re-recorded.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// What a finished command left behind, reduced to what a user sees.
#[derive(Debug, PartialEq, Eq)]
pub struct Seen {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl From<Output> for Seen {
    fn from(out: Output) -> Self {
        Seen {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

/// Write `contents` at `rel` under `dir`, creating directories.
pub fn write(dir: &Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, contents).expect("write");
}

/// A recall-daemon socket that answers the client's `Ping` and then the first
/// `Recall` with `answer`: the wire contract with no corpus behind it. The
/// caller joins the handle and removes the socket the thread leaves behind.
pub fn fake_recall_daemon(
    recall_dir: &Path,
    answer: pixel_daemon::Response,
) -> std::thread::JoinHandle<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    let listener = UnixListener::bind(pixel_daemon::daemon::socket_path(recall_dir)).expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    std::thread::spawn(move || {
        // Poll with a deadline: a client that never reaches the `Recall` must
        // fail its own assertion, not hang here.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut served = false;
        while !served && std::time::Instant::now() < deadline {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            };
            // A non-blocking listener hands back a non-blocking socket on
            // BSD: reset it, or the read races the client's write.
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            while !served {
                let mut line = String::new();
                let Ok(n) = reader.read_line(&mut line) else {
                    break;
                };
                if n == 0 {
                    break;
                }
                let reply = match serde_json::from_str::<pixel_daemon::Request>(&line)
                    .expect("a request")
                {
                    pixel_daemon::Request::Ping => pixel_daemon::Response::success(
                        "ping",
                        serde_json::json!({"pong": true, "protocol_version": pixel_daemon::api::PROTOCOL_VERSION}),
                    ),
                    pixel_daemon::Request::Recall { .. } => {
                        served = true;
                        answer.clone()
                    }
                    other => panic!("unexpected request: {other:?}"),
                };
                writeln!(stream, "{}", serde_json::to_string(&reply).expect("json"))
                    .expect("write");
            }
        }
    })
}

/// The core `pixel` binary named by `PIXEL_CORE_BIN`, when set.
pub fn core_bin() -> Option<PathBuf> {
    std::env::var_os("PIXEL_CORE_BIN")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The machine-wide state directory every command under test shares:
/// core keeps machine-wide state under `$XDG_STATE_HOME/pixel`, and a
/// test must neither read nor write the user's.
fn state_home() -> &'static Path {
    static STATE: std::sync::OnceLock<TempDir> = std::sync::OnceLock::new();
    STATE
        .get_or_init(|| tempfile::tempdir().expect("tempdir"))
        .path()
}

/// Run `bin args…` in `cwd` with the host variables cleared, plus `envs`.
pub fn run(bin: &Path, args: &[&str], cwd: &Path, envs: &[(&str, &str)]) -> Seen {
    let mut command = Command::new(bin);
    command
        .args(args)
        .current_dir(cwd)
        .env_remove("PIXEL_REPO_ROOT")
        .env_remove("PIXEL_GRAPH_DB")
        .env_remove("PIXEL_BIN")
        .env_remove("PIXEL_API")
        .env_remove("PIXEL_RECALL_MODEL")
        .env_remove("PIXEL_RECALL_DIR")
        .env("XDG_STATE_HOME", state_home())
        .env("PIXEL_METRICS", "0");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("command runs").into()
}

// ---------------------------------------------------------------------------
// Parity with the built-in command
// ---------------------------------------------------------------------------

/// One invocation to compare, built fresh for each side so a command that
/// mutates its repository runs against an untouched copy both times.
pub struct Setup {
    /// The working directory.
    pub dir: TempDir,
    /// `before` commands (arguments after `pixel <command>`), run first with
    /// the same implementation, e.g. `index` ahead of `search`.
    pub before: Vec<Vec<String>>,
    /// The arguments after `pixel <command>`.
    pub args: Vec<String>,
    /// Extra environment, e.g. `PIXEL_FLOW_DIR`.
    pub envs: Vec<(String, String)>,
    /// Other directories whose paths must be hidden from the comparison.
    pub scrub_paths: Vec<PathBuf>,
    /// Scratch directories kept alive until the setup is dropped.
    keep: Vec<TempDir>,
}

impl Setup {
    /// `args` run in `dir`, with nothing before them.
    pub fn new(dir: TempDir, args: &[&str]) -> Self {
        Setup {
            dir,
            before: Vec::new(),
            args: args.iter().map(ToString::to_string).collect(),
            envs: Vec::new(),
            scrub_paths: Vec::new(),
            keep: Vec::new(),
        }
    }

    /// A fresh scratch directory that lives as long as the setup and is
    /// hidden from the comparison.
    pub fn scratch(&mut self) -> PathBuf {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        self.scrub_paths.push(path.clone());
        self.keep.push(dir);
        path
    }

    /// A command to run first.
    pub fn then(mut self, before: &[&str]) -> Self {
        self.before
            .push(before.iter().map(ToString::to_string).collect());
        self
    }

    /// An environment variable for every command of this setup.
    pub fn env(mut self, key: &str, value: impl AsRef<Path>) -> Self {
        self.envs
            .push((key.to_string(), value.as_ref().display().to_string()));
        self
    }

    /// A directory to hide from the comparison.
    pub fn scrub(mut self, path: &Path) -> Self {
        self.scrub_paths.push(path.to_path_buf());
        self
    }
}

/// A fresh empty directory to run in.
pub fn scratch_dir() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// Replace every run of 7 or more lowercase hex digits that stands alone
/// (object ids, short ids, unix timestamps) with `OID`.
pub fn scrub_ids(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let is_hex = |c: char| c.is_ascii_digit() || ('a'..='f').contains(&c);
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let standalone_start = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if is_hex(c) && standalone_start {
            let mut j = i;
            while j < chars.len() && is_hex(chars[j]) {
                j += 1;
            }
            let standalone_end = j == chars.len() || !chars[j].is_ascii_alphanumeric();
            if j - i >= 7 && standalone_end {
                out.push_str("OID");
                i = j;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// What a user saw, with the volatile parts replaced: each directory by a
/// label, ids and timestamps by `OID`, blank stderr lines dropped.
fn render(seen: &Seen, setup: &Setup) -> String {
    let mut text = format!(
        "exit: {}\n--- stdout\n{}--- stderr\n{}",
        seen.code,
        seen.stdout,
        seen.stderr
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| format!("{line}\n"))
            .collect::<String>()
    );
    let mut dirs: Vec<(&Path, &str)> = vec![(setup.dir.path(), "ROOT")];
    dirs.extend(
        setup
            .scrub_paths
            .iter()
            .map(|path| (path.as_path(), "EXTRA")),
    );
    for (dir, label) in dirs {
        for form in [dir.canonicalize().ok(), Some(dir.to_path_buf())]
            .into_iter()
            .flatten()
        {
            text = text.replace(&form.display().to_string(), label);
        }
    }
    scrub_volatile(&scrub_millis(&scrub_snapshot_ids(&scrub_ids(
        &drop_metrics_option(&text),
    ))))
}

/// Replace what depends on the clock or the platform and is not a contract:
/// the wall-clock minute of a last ingest, the size of the database file.
pub fn scrub_volatile(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("last ingest ") {
            // `YYYY-MM-DD HH:MM`
            let stamp = tail.get(..16).filter(|stamp| {
                stamp.bytes().enumerate().all(|(i, b)| match i {
                    4 | 7 => b == b'-',
                    10 => b == b' ',
                    13 => b == b':',
                    _ => b.is_ascii_digit(),
                })
            });
            if stamp.is_some() {
                out.push_str("last ingest DATE");
                rest = &tail[16..];
                continue;
            }
        }
        if let Some(tail) = rest.strip_prefix("\"db_bytes\":") {
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                out.push_str("\"db_bytes\":N");
                rest = &tail[digits..];
                continue;
            }
        }
        if let Some(tail) = rest.strip_prefix(" (") {
            // ` (0.1 MB)`
            let int = tail.bytes().take_while(u8::is_ascii_digit).count();
            let frac = tail[int..]
                .strip_prefix('.')
                .map_or(0, |f| f.bytes().take_while(u8::is_ascii_digit).count());
            if int > 0 && frac > 0 && tail[int + 1 + frac..].starts_with(" MB)") {
                out.push_str(" (N MB)");
                rest = &tail[int + 1 + frac + 4..];
                continue;
            }
        }
        let ch = rest.chars().next().expect("non-empty");
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// Replace every `<digits> ms` duration with `N ms`: how long an index pass
/// took is not part of the contract.
pub fn scrub_millis(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_ascii_alphanumeric()) {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            let rest: String = chars[j..].iter().take(3).collect();
            let after = chars.get(j + 3);
            if rest == " ms" && after.is_none_or(|c| !c.is_ascii_alphanumeric()) {
                out.push('N');
                i = j;
                continue;
            }
            out.extend(&chars[i..j]);
            i = j;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Remove the `--metrics` option from a `--help` text. The built-in lists it
/// (the live-metrics line belongs to the `pixel` binary); the plugin accepts
/// it hidden. The one documented difference in help output.
pub fn drop_metrics_option(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.split_inclusive('\n');
    while let Some(line) = lines.next() {
        let Some(option) = line.trim_start().strip_prefix("--metrics <METRICS>") else {
            out.push_str(line);
            continue;
        };
        // Long help: the option stands alone and a paragraph follows, up to a
        // blank line. Short help: everything is on this line.
        if option.trim().is_empty() {
            for next in lines.by_ref() {
                // The separator is empty; a paragraph break inside the option
                // is indented whitespace.
                if next.trim_end_matches('\n').is_empty() {
                    break;
                }
            }
        }
    }
    out
}

/// Replace every snapshot id (`20261013T232719.415573000Z`, a UTC timestamp to
/// the nanosecond) with `SNAPSHOT`.
pub fn scrub_snapshot_ids(text: &str) -> String {
    const SHAPE: &[u8] = b"ddddddddTdddddd.dddddddddZ";
    let bytes = text.as_bytes();
    let fits = |at: usize| {
        bytes.len() >= at + SHAPE.len()
            && SHAPE.iter().enumerate().all(|(i, shape)| match shape {
                b'd' => bytes[at + i].is_ascii_digit(),
                other => bytes[at + i] == *other,
            })
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if fits(i) {
            out.push_str("SNAPSHOT");
            i += SHAPE.len();
        } else {
            let ch = text[i..].chars().next().expect("char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Run `bin` through a [`Setup`]; `prefix` is the subcommand words in front
/// of the arguments: `["recall"]` for both `pixel recall` and `pixel-recall recall`.
fn execute(bin: &Path, prefix: &[&str], setup: &Setup) -> String {
    let envs: Vec<(&str, &str)> = setup
        .envs
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    for before in &setup.before {
        let args: Vec<&str> = prefix
            .iter()
            .copied()
            .chain(before.iter().map(String::as_str))
            .collect();
        let seen = run(bin, &args, setup.dir.path(), &envs);
        assert_eq!(
            seen.code, 0,
            "setup step {before:?} failed: {}",
            seen.stderr
        );
    }
    let args: Vec<&str> = prefix
        .iter()
        .copied()
        .chain(setup.args.iter().map(String::as_str))
        .collect();
    render(&run(bin, &args, setup.dir.path(), &envs), setup)
}

/// The parity check of one case.
///
/// `command` is the built-in's subcommand name. `make` builds a fresh
/// [`Setup`]; it is called once for the plugin and once for the built-in.
///
/// - the plugin's rendering is compared with `tests/golden/<case>.txt`;
/// - with `PIXEL_CORE_BIN` set it is also compared with the live built-in;
/// - with `UPDATE_GOLDEN=1` (which requires `PIXEL_CORE_BIN`) the golden is
///   rewritten from the **built-in**'s rendering, then compared as usual.
pub fn assert_parity(
    manifest_dir: &str,
    plugin_bin: &str,
    command: &str,
    case: &str,
    make: &dyn Fn() -> Setup,
) {
    parity(manifest_dir, plugin_bin, command, case, make, true);
}

/// [`assert_parity`] against the golden only, never the live built-in: for a
/// command that already left core, whose goldens are history.
pub fn assert_golden(
    manifest_dir: &str,
    plugin_bin: &str,
    command: &str,
    case: &str,
    make: &dyn Fn() -> Setup,
) {
    parity(manifest_dir, plugin_bin, command, case, make, false);
}

fn parity(
    manifest_dir: &str,
    plugin_bin: &str,
    command: &str,
    case: &str,
    make: &dyn Fn() -> Setup,
    live: bool,
) {
    let plugin = execute(Path::new(plugin_bin), &[command], &make());
    let golden_path = Path::new(manifest_dir)
        .join("tests/golden")
        .join(format!("{case}.txt"));
    let core = core_bin()
        .filter(|_| live)
        .map(|bin| execute(&bin, &[command], &make()));
    if live && std::env::var_os("UPDATE_GOLDEN").is_some() {
        let core = core.as_ref().expect("UPDATE_GOLDEN needs PIXEL_CORE_BIN");
        std::fs::create_dir_all(golden_path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&golden_path, core).expect("write golden");
    }
    let golden = std::fs::read_to_string(&golden_path).unwrap_or_else(|_| {
        panic!(
            "missing golden {} (record it: PIXEL_CORE_BIN=<core pixel> UPDATE_GOLDEN=1 cargo test)",
            golden_path.display()
        )
    });
    assert_eq!(
        plugin, golden,
        "{case}: plugin differs from the recorded built-in"
    );
    if let Some(core) = core {
        assert_eq!(
            plugin, core,
            "{case}: plugin differs from the live built-in"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_ids_become_a_label() {
        assert_eq!(
            scrub_snapshot_ids("{\"id\":\"20261013T232719.415573000Z\",\"é\":1}"),
            "{\"id\":\"SNAPSHOT\",\"é\":1}"
        );
        assert_eq!(scrub_snapshot_ids("20261013T2327.4Z"), "20261013T2327.4Z");
    }

    #[test]
    fn clock_and_platform_dependent_state_is_scrubbed() {
        assert_eq!(
            scrub_volatile(
                "corpus: /r (0.1 MB)\nclaude 1 last ingest 2026-10-13 23:59\n{\"db_bytes\":61440,\"x\":1}"
            ),
            "corpus: /r (N MB)\nclaude 1 last ingest DATE\n{\"db_bytes\":N,\"x\":1}"
        );
        assert_eq!(
            scrub_volatile("last ingest never (a MB)"),
            "last ingest never (a MB)"
        );
    }

    #[test]
    fn a_duration_in_milliseconds_becomes_n() {
        assert_eq!(
            scrub_millis("0 errors, 12 ms\n48 ms (1 total)"),
            "0 errors, N ms\nN ms (1 total)"
        );
        assert_eq!(scrub_millis("v2 msg 3 msec 7"), "v2 msg 3 msec 7");
    }

    #[test]
    fn the_metrics_option_is_dropped_in_both_help_styles() {
        let long = "Options:\n      --metrics <METRICS>\n          Disable\n          \n          [default: on]\n\n      --json\n          \n";
        assert_eq!(
            drop_metrics_option(long),
            "Options:\n      --json\n          \n"
        );
        let short =
            "Options:\n      --metrics <METRICS>  Disable [default: on]\n  -h, --help  Print\n";
        assert_eq!(
            drop_metrics_option(short),
            "Options:\n  -h, --help  Print\n"
        );
    }

    #[test]
    fn standalone_hex_runs_of_seven_or_more_become_oid() {
        assert_eq!(
            scrub_ids("at 59cbad6 and 59cbad66bb53960ff2b9719a24b969586680b3e5."),
            "at OID and OID."
        );
        assert_eq!(scrub_ids("t=1791651110;"), "t=OID;");
        // Words and short numbers stay.
        assert_eq!(
            scrub_ids("facade 1234567x abc 123456"),
            "facade 1234567x abc 123456"
        );
    }
}
