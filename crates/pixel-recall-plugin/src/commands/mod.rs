// SPDX-FileCopyrightText: The Pixel contributors
// SPDX-License-Identifier: MIT

//! The one command of the `recall` plugin.

use std::ffi::OsString;
use std::process::ExitCode;

mod recall;

/// One command: the name `pixel <name>` is typed with, a line for `--help`,
/// and the entry point, which receives the arguments after the name.
pub struct Command {
    pub name: &'static str,
    pub about: &'static str,
    run: fn(Vec<OsString>) -> ExitCode,
}

impl Command {
    /// Run the command on `args` (the arguments after its name).
    pub fn run(&self, args: &[OsString]) -> ExitCode {
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(OsString::from(format!("pixel {}", self.name)));
        argv.extend_from_slice(args);
        (self.run)(argv)
    }
}

/// Every command. This list is also the `commands` array of
/// `pixel-plugin.toml` (`tests/manifest.rs` compares the two).
pub const COMMANDS: [Command; 1] = [Command {
    name: "recall",
    about: "Search and browse LLM CLI transcripts (machine-wide corpus)",
    run: recall::recall,
}];
