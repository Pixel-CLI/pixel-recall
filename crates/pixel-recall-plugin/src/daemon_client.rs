// SPDX-FileCopyrightText: The Pixel contributors
// SPDX-License-Identifier: MIT

//! The client half of the recall daemon: probe it, ask it, stop it.
//!
//! Ported from `crates/pixel/src/main.rs` of Pixel-CLI/pixel (MIT) at
//! `ad788194`, where these functions are private to the `pixel` binary:
//! `roundtrip`, `open_daemon_stream`, `DaemonProbe` / `classify_ping` /
//! `probe_daemon`, `retire_stale_daemon_within`, `DaemonRoute` /
//! `try_daemon_inner` / `send_to_daemon`, `daemon_ping`, `daemon_stop`,
//! `daemon_status`. Only the recall corpus uses them here, so everything that
//! served the repository daemon (auto-start, the stale-protocol grace for a
//! repository `Service`) is not carried over; the wire (`pixel_daemon::api`)
//! and the daemon itself (`pixel_daemon::daemon::run_corpus`) are core's.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use pixel_daemon::api::{PROTOCOL_VERSION, Request, Response};
use pixel_daemon::daemon;
use serde_json::Value;

use crate::output::write_stdout;

/// One NDJSON round trip on an open stream.
fn roundtrip(stream: &mut UnixStream, req: &Request) -> Option<Response> {
    let mut line = serde_json::to_string(req).ok()?;
    line.push('\n');
    stream.write_all(line.as_bytes()).ok()?;
    stream.flush().ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut buf = String::new();
    reader.read_line(&mut buf).ok()?;
    serde_json::from_str(&buf).ok()
}

fn open_daemon_stream(root: &Path) -> Option<UnixStream> {
    let sock = daemon::socket_path(root);
    let stream = UnixStream::connect(&sock).ok()?;
    // The daemon drains its debounced watcher batch before serving a
    // connection; on a cold or loaded host that drain can outlast a short
    // probe timeout. 5s covers the drain without masking a dead daemon.
    stream
        .set_read_timeout(Some(Duration::from_millis(5000)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(5000)))
        .ok()?;
    Some(stream)
}

/// What answers on a root's daemon socket, from this client's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonProbe {
    /// Nothing answers a Ping.
    Absent,
    /// A healthy daemon on this client's protocol.
    Current,
    /// An older protocol, no protocol at all, or a failed Ping: it must not
    /// serve this client, and every daemon ever shipped understands Shutdown.
    Stale,
    /// A newer protocol: a newer client started it and may still be using it.
    Newer,
}

fn classify_ping(ping: &Response) -> DaemonProbe {
    match ping.data().get("protocol_version").and_then(Value::as_u64) {
        Some(version) if version > PROTOCOL_VERSION => DaemonProbe::Newer,
        Some(PROTOCOL_VERSION) if ping.ok => DaemonProbe::Current,
        _ => DaemonProbe::Stale,
    }
}

pub fn probe_daemon(root: &Path) -> DaemonProbe {
    open_daemon_stream(root)
        .and_then(|mut stream| roundtrip(&mut stream, &Request::Ping))
        .map_or(DaemonProbe::Absent, |ping| classify_ping(&ping))
}

/// How long a stale daemon gets to release its socket after Shutdown.
const STALE_DAEMON_EXIT_CAP: Duration = Duration::from_secs(2);

/// Shut a stale daemon down and wait (up to `cap`) until it stops
/// answering, so a replacement started right after does not die on the
/// lock the old one still holds.
fn retire_stale_daemon_within(root: &Path, cap: Duration) {
    if let Some(mut stream) = open_daemon_stream(root) {
        let _ = roundtrip(&mut stream, &Request::Shutdown);
    }
    let deadline = std::time::Instant::now() + cap;
    while std::time::Instant::now() < deadline && probe_daemon(root) == DaemonProbe::Stale {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Outcome of routing one request through the daemon.
#[derive(Debug)]
pub enum DaemonRoute {
    Served(Box<Response>),
    /// No daemon can serve it now.
    Absent,
    /// A newer daemon is running: serve in process, start nothing.
    Declined,
}

pub fn try_daemon_inner(root: &Path, req: &Request) -> DaemonRoute {
    probe_route(root).unwrap_or_else(|| send_to_daemon(root, req))
}

/// The route the probe settles by itself; `None` when a current daemon
/// answered and the request can be sent to it.
fn probe_route(root: &Path) -> Option<DaemonRoute> {
    match probe_daemon(root) {
        DaemonProbe::Current => None,
        DaemonProbe::Newer => Some(DaemonRoute::Declined),
        DaemonProbe::Stale => {
            // An old daemon must not serve stale schemas to a newer client.
            retire_stale_daemon_within(root, STALE_DAEMON_EXIT_CAP);
            Some(DaemonRoute::Absent)
        }
        DaemonProbe::Absent => Some(DaemonRoute::Absent),
    }
}

/// Send `req` to the daemon a probe found current.
fn send_to_daemon(root: &Path, req: &Request) -> DaemonRoute {
    // Each request gets its own connection. Besides removing a head-of-line
    // wait behind the Ping handshake, this keeps the client compatible with
    // a daemon that intentionally serves one request per connection.
    let served = open_daemon_stream(root).and_then(|mut stream| {
        // Real request may legitimately take a while (a model load).
        stream
            .set_read_timeout(Some(Duration::from_secs(600)))
            .ok()?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .ok()?;
        roundtrip(&mut stream, req)
    });
    served.map_or(DaemonRoute::Absent, |response| {
        DaemonRoute::Served(Box::new(response))
    })
}

/// Probe a daemon without starting one.
///
/// A stale one is retired here, so the start that usually follows a `false`
/// is not refused by the lock it still holds.
pub fn daemon_ping(root: &Path) -> bool {
    match probe_daemon(root) {
        DaemonProbe::Current => true,
        DaemonProbe::Stale => {
            retire_stale_daemon_within(root, STALE_DAEMON_EXIT_CAP);
            false
        }
        DaemonProbe::Absent | DaemonProbe::Newer => false,
    }
}

pub fn daemon_stop(path: PathBuf) -> Result<(), String> {
    // Straight to the socket: every protocol understands Shutdown.
    let stopped =
        open_daemon_stream(&path).and_then(|mut stream| roundtrip(&mut stream, &Request::Shutdown));
    match stopped {
        Some(r) if r.ok => {
            write_stdout("daemon stopped\n")?;
            Ok(())
        }
        _ => {
            write_stdout(&format!("no daemon running for {}\n", path.display()))?;
            Ok(())
        }
    }
}

pub fn daemon_status(path: PathBuf) -> Result<(), String> {
    if daemon_ping(&path) {
        write_stdout(&format!(
            "daemon running ({})\n",
            daemon::socket_path(&path).display()
        ))?;
    } else {
        write_stdout(&format!("daemon not running for {}\n", path.display()))?;
    }
    Ok(())
}

/// The wall clock in milliseconds since the Unix epoch
/// (`pixel_actionlog::now_ms` in core).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
