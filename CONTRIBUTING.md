# Contributing

The `recall` plugin of [Pixel](https://github.com/Pixel-CLI/pixel). Read the
[README](README.md) first.

## Before a pull request

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
scripts/check-pin.sh
```

CI runs the same on Ubuntu and macOS. Every source file opens with
`SPDX-FileCopyrightText: The Pixel contributors` and
`SPDX-License-Identifier: MIT`.

## A command keeps the built-in's behaviour

A command reproduces the one it replaced: same arguments, same output, same
exit status. `crates/pixel-recall-plugin/tests/parity_recall.rs` runs
`pixel-recall` on a scratch corpus and compares the result with a recorded
golden in `tests/golden/`. Goldens are recorded from the **built-in** command,
never from this plugin.

The goldens were recorded from a core built at `ad788194`, the pinned commit
(`cargo build -p pixel-cli` in a checkout of it). The corpus is one Claude
transcript under a throwaway `HOME`; the clock and the platform are scrubbed
(`N ms`, `last ingest DATE`, `db_bytes`, ids and unix times become `OID`,
directories `ROOT`/`EXTRA`). The one known difference, the `--metrics` option
in `--help`, is dropped from both sides. No case needs the embedding model:
`ask` and `context` run `--lexical-only`.

Re-record after a change, or after moving the pin:

```bash
PIXEL_CORE_BIN=/path/to/pixel UPDATE_GOLDEN=1 cargo test --workspace
git diff crates/pixel-recall-plugin/tests/golden   # every change is a behaviour change: explain it
```

Running the tests with `PIXEL_CORE_BIN` set and without `UPDATE_GOLDEN` also
compares the plugin with that live built-in. CI sets neither.

`tests/cli.rs` starts a real corpus daemon from the built binary, asks it and
stops it, and drives a fake daemon on the socket for the failure path: the
wire is core's, so a pin bump that changes `PROTOCOL_VERSION` shows there.

A deliberate difference from the built-in has no golden: test it directly and
list it under "Differences from the built-ins" in the README.

## Moving the Pixel pin

The pinned commit lives only in `[workspace.dependencies]` of `Cargo.toml`.
Change `rev` on every entry together, run `cargo update`, then
`cargo test --workspace`. `tests/core_drift.rs` fails when something this
repository restates (the failure-envelope classifier) no longer matches core.

## Adding a subcommand

1. A variant of `RecallCmd` in `src/commands/recall.rs`, its `run_*` function
   and the arm in `run_recall`; print with `output::print_data` or
   `output::write_stdout`.
2. A case in `tests/parity_recall.rs` and its golden.
3. When the subcommand reaches the daemon, go through `routed_to_recall_daemon`
   and keep the in-process fallback.

## Releasing

Bump `version` in `Cargo.toml` (`[workspace.package]`) and `pixel-plugin.toml`
together, merge to `main`, then tag `vX.Y.Z` on `main`. `release.yml` checks
the tag against both files, creates the release and uploads
`pixel-recall-<target-triple>` with its `.sha256` for four targets.
