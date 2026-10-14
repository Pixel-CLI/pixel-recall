# pixel-recall

The `recall` plugin for [Pixel](https://github.com/Pixel-CLI/pixel):
`pixel recall`, machine-wide search over the transcripts of every agent CLI
(Claude Code, Codex, Cursor, Devin, Gemini, opencode, pi, Z Code), in one
`pixel-recall` binary. It used to be built into `pixel`; it lives here so the
core stays small and only the people who search their sessions install it.

| Command | What it does |
| --- | --- |
| `pixel recall index` | ingest transcript sources into the corpus (incremental; `--full` rebuilds the lexical segments) |
| `pixel recall search <pattern>` | regex search over every indexed turn, newest first (`--agent`, `--repo`, `--since`, `--until`, `--role`, `--human-only`, `--session`, `--word`) |
| `pixel recall sessions` | list indexed sessions, newest first |
| `pixel recall show <ref>` | print a session's turns (`--turn N..M`) |
| `pixel recall ask <query>` | natural-language hybrid search (lexical + semantic), grouped by session |
| `pixel recall context <query>` | token-budgeted context pack for a query (`--budget`) |
| `pixel recall export --out <dir>` | bulk-export sessions, one file each (`md` or `jsonl`) |
| `pixel recall setup` / `embed` | download the embedding model once; embed pending turns (resumable) |
| `pixel recall status` | corpus freshness, counts and location |
| `pixel recall daemon start\|stop\|status` | the corpus daemon: watches every CLI's transcript store, keeps the corpus and embeddings fresh, serves warm-model `ask` |

Each subcommand reproduces the built-in's arguments, output and exit status.
`pixel recall --help` lists them; `pixel recall <subcommand> --help` shows one's
options.

## Install

With a Pixel that has the plugin host (`pixel install <source>`):

```bash
pixel install github:Pixel-CLI/pixel-recall
pixel plugin enable recall     # the plugin declares network access (model download)
```

The release assets are `pixel-recall-<target-triple>` executables, each with a
`.sha256` file: `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`.

From source, without Pixel's plugin management:

```bash
cargo install --git https://github.com/Pixel-CLI/pixel-recall pixel-recall-plugin
```

`pixel recall …` finds the binary through the plugin (the host runs
`bin/pixel-recall`, which is, or finds, the `pixel-recall` binary) or, for a
source install, through `pixel-recall` on `PATH`. `pixel-recall recall …` is the
same command line.

## The corpus and its daemon

Everything lives under `PIXEL_RECALL_DIR` (default
`~/.local/share/pixel/recall`): the SQLite corpus, the lexical segments, the
vectors and the models. A query catches up on demand (a bounded scan of each
agent's transcripts since its last ingest), so `recall index` is optional.

`pixel recall daemon start` runs `pixel-recall daemon start --foreground` in the
background. The daemon speaks core's wire protocol (`pixel_daemon::api`) and is
interchangeable with the one `pixel recall daemon start` ran when the command
was built in: either binary can start, query and stop a daemon the other
started, as long as both pin a core commit with the same `PROTOCOL_VERSION`.
Features `fastembed` (ONNX) and `model2vec` (pure Rust) are on by default, as in
`pixel`; `--no-default-features --features model2vec` builds for musl.

## Environment

The host passes `PIXEL_API`, `PIXEL_REPO_ROOT`, `PIXEL_GRAPH_DB` and
`PIXEL_BIN`; none is read, a corpus belongs to the machine and not to a
repository. `PIXEL_RECALL_DIR`, `PIXEL_RECALL_MODEL` and
`PIXEL_OUTPUT_CAP_BYTES` behave as they did in the built-in.

## Differences from the built-in

- **No metrics line, no action-log entry.** A plugin is a separate process and
  the invocation accounting belongs to the `pixel` binary, including the line
  that said how each recall request was served (daemon or in process, and where
  the milliseconds went). `--metrics on|off` is still accepted (hidden), so
  scripts that pass it keep parsing; `--help` does not list it and a usage line
  omits `[OPTIONS]` where `--metrics` was the only option.
- **The daemon restart command line.** The daemon is re-run as
  `pixel-recall daemon start --foreground`, not `pixel recall daemon start
  --foreground`; a daemon started by one is found and stopped by the other.

## Where the code comes from

Library crates of Pixel core (`pixel-recall`, `pixel-daemon`, `pixel-proto`)
come through a git dependency pinned to one commit of
[Pixel-CLI/pixel](https://github.com/Pixel-CLI/pixel), recorded once in
`[workspace.dependencies]` of [`Cargo.toml`](Cargo.toml)
(`scripts/check-pin.sh` enforces a single value). Logic that lived only in the
`pixel` binary crate is ported here, MIT, with its origin in each module's doc:

| Here | From |
| --- | --- |
| `commands/recall.rs` | `crates/pixel/src/recall_cmd.rs` (command lines, handlers, the context pack, lazy catch-up, unit tests) |
| `daemon_client.rs` | the daemon client of `crates/pixel/src/main.rs`: `roundtrip`, `probe_daemon`, `try_daemon_inner`, `daemon_ping`, `daemon_stop`, `daemon_status` |
| `output.rs`, `run.rs` | `print_data` with its byte cap (`PIXEL_OUTPUT_CAP_BYTES`), the `--json` failure envelope |

## Versioning

One version (`[workspace.package]`), equal to the `version` in
`pixel-plugin.toml`; a release is a `vX.Y.Z` tag on `main`, which builds the
assets above. `api = 1` changes only when Pixel changes the plugin API. Moving
the Pixel pin to a commit with another `PROTOCOL_VERSION` is a minor version:
the daemon of the old and the new binary no longer replace each other.

## License

MIT, see [LICENSE](LICENSE).
