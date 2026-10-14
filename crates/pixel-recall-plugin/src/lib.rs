// SPDX-FileCopyrightText: The Pixel contributors
// SPDX-License-Identifier: MIT

//! The `recall` plugin of Pixel: machine-wide search over the transcripts of
//! every agent CLI, behind the `pixel-recall` binary (plugin API 1).
//!
//! A plugin is a separate executable that Pixel core runs for `pixel <name> …`.
//! This one answers to a single name, so `pixel recall search …` is
//! `pixel-recall search …`: [`dispatch`] hands every argument to the command.
//!
//! The corpus, its indexes, the transcript adapters and the corpus daemon are
//! core's `pixel-recall` and `pixel-daemon` library crates. Ported here is
//! what was private to the `pixel` binary: the command line, the printing and
//! the daemon client ([`daemon_client`]).

pub mod commands;
pub mod daemon_client;
pub mod output;
pub mod run;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

use std::ffi::OsString;
use std::process::ExitCode;

/// Run the plugin for `args` (the whole argv, program name first).
pub fn dispatch(args: &[OsString]) -> ExitCode {
    if args.len() == 2 && matches!(args[1].to_str(), Some("-V" | "--version")) {
        println!("pixel-recall {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let [command] = &commands::COMMANDS;
    let rest = args.get(1..).unwrap_or_default();
    // `pixel-recall recall search …` is `pixel-recall search …`: no
    // subcommand is named `recall`.
    let rest = match rest.first().and_then(|arg| arg.to_str()) {
        Some(name) if name == command.name => &rest[1..],
        _ => rest,
    };
    command.run(rest)
}
