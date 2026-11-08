# A Pixel plugin's README

The standard for the README of a Pixel plugin (`pixel <name>` backed by a
plugin repository). One README per plugin repository; every section below
appears, in this order. A section the plugin genuinely has no content for
stays, with one honest line saying so — a reader should not wonder whether
it was forgotten.

A plugin README answers, in order: what is this, what does it run, how do I
install it, how do I configure it, what data does it touch, how does it
differ from what was built in, and where did the code come from.

## 1. Title and one-line what-it-is

`# <repo name>` and one short paragraph: the plugin's name as Pixel runs it,
what it does, and the one constraint that explains why it is a plugin (the
core stays small; only the people who need it install it). If the command was
once built into `pixel`, say so here — that is the first question a reader has.

## 2. Commands

A table: one row per subcommand (`pixel <name> <sub>`), what it does, and its
options in one parenthesised list. Keep it scannable; `--help` is the source
of truth for the details. After the table, one line on how `--help` behaves
per subcommand.

## 3. Install

How to install through Pixel's plugin host (`pixel install <source>`,
`pixel plugin enable <name>` when the manifest declares network), the release
assets and their target triples, and the source install without Pixel's
plugin management (`cargo install --git …`). End with the line on how
`pixel <name> …` finds the binary.

## 4. Data and configuration

Where the plugin keeps its state (a `PIXEL_<NAME>_DIR` default under
`~/.local/share/pixel/`), every environment variable it reads, and what the
host passes that it does **not** read. A long-running component (a daemon)
gets its own sub-section: what it watches, how it starts, and the wire
protocol / version compatibility rule.

## 5. Differences from the built-in

A bullet list, one per deliberate difference from the command as it was
built into `pixel` (or, for a plugin that never was built in: what it is
tested against). Empty only when the plugin was never built in.

## 6. Where the code comes from

Library crates of Pixel core through the git dependency, the pinned commit
recorded once in `[workspace.dependencies]`, and a table mapping this
repository's modules to the files they were ported from. A plugin that is
new code says so in one line and links its issue or design doc.

## 7. Versioning

One version across `[workspace.package]`, `pixel-plugin.toml` and the tag
(`vX.Y.Z` on `main`, which builds the release assets). What bumps the minor
version (`api =` changes, a `PROTOCOL_VERSION` change in the pin).

## 8. License

One line, linking `LICENSE`. Match the SPDX headers in every source file.

## Privacy

Not a numbered section — a paragraph inside "4. Data and configuration": what
the plugin reads (files, transcripts, logs), what never leaves the machine,
and the one exception (a model download) with the switch that disables it.
A plugin README that does not say what data it touches is a defect, not an
omission.

## Template

```markdown
# <repo>

The `<name>` plugin for [Pixel](https://github.com/Pixel-CLI/pixel):
`pixel <name>`, <what it does>, in one `<binary>` binary. <Why it is a
plugin: the core stays small; …>. <It used to be built into `pixel`; …>

| Command | What it does |
| --- | --- |
| `pixel <name> <sub>` | <what> (`<options>`) |

## Install

<`pixel install …`, `pixel plugin enable …`, assets + triples, source
install, how `pixel <name>` finds the binary>

## Data and configuration

<`PIXEL_<NAME>_DIR`, env vars read, host env not read, daemon sub-section,
privacy paragraph>

## Differences from the built-in

- <one per difference, or "never built in">

## Where the code comes from

<git dependency, pin, origin table — or "new code, see #N">

## Versioning

<one version, tag, what bumps the minor>

## License

MIT, see [LICENSE](LICENSE).
```

`pixel-recall`'s [README](../README.md) is the first conforming example.