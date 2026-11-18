<div align="center">

# rpt-tui

**A terminal UI for your [rpt](https://github.com/astr4liteEx/rpt) report.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust 1.99+](https://img.shields.io/badge/rust-1.99%2B-orange.svg)
![Linux amd64 | arm64](https://img.shields.io/badge/platform-linux%20amd64%20%7C%20arm64-lightgrey.svg)

[Features](#features) • [Install](#install) • [Usage](#usage) • [Configure](#configure) • [Development](#development)

</div>

rpt-tui opens on one rpt report and shows its findings and problems side by side with everything you run against it: rpt commands, plugins, your own chains of commands and your editor. Each run is a real terminal inside a pane, so a plugin can draw its own menus and pickers.

![The home screen: findings, problems and runs on the left, the selected item on the right](docs/demo/screen.gif)

> [!IMPORTANT]
> rpt-tui never writes report files itself. Every change goes through rpt, a plugin or your editor.

## Features

- **Findings and problems at a glance**: findings sorted most severe first using the report's own severity levels, and what `rpt validate` reports, refreshed after every run.
- **Command palette**: rpt built-ins, plugins, your chains and rpt-tui's own actions, filtered as you type.
- **Live runs in a pane**: real terminals that can go full screen, kept with their transcripts across sessions.
- **Chains**: your own multi-step commands, with parameters asked for in a form and an optional hotkey.
- **Selection-aware**: every run gets the selected finding or problem in its environment; `e` opens it in your editor at the right line.
- **Notifications**: toasts for runs that end out of sight, and desktop notifications for long ones.
- **Configurable**: three built-in themes or your own, rebindable keys, a forgiving config that never blocks startup.

## Install

rpt-tui runs on Linux (amd64 and arm64) and needs `rpt` on your `PATH`.

### From a release

Download the archive for your machine and `SHA256SUMS` from the [releases](https://github.com/astr4liteEx/rpt-tui/releases), then check, unpack and install:

```sh
sha256sum -c --ignore-missing SHA256SUMS
tar -xzf rpt-tui_v0.1.0_linux_amd64.tar.gz
install rpt-tui ~/.local/bin/
```

The binaries are static, so they run on any Linux.

### With cargo

Requires Rust 1.99 or newer:

```sh
cargo install --git https://github.com/astr4liteEx/rpt-tui --tag v0.1.0 --locked
```

> [!TIP]
> For a static binary, add the musl target and link with Rust's own linker:
>
> ```sh
> rustup target add x86_64-unknown-linux-musl
> CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
>   cargo install --git https://github.com/astr4liteEx/rpt-tui --tag v0.1.0 --locked \
>   --target x86_64-unknown-linux-musl
> ```

## Usage

From inside a report folder (one holding `rpt.yaml`, or anywhere below it):

```sh
rpt tui
```

rpt finds `rpt-tui` on your `PATH` as a plugin; running `rpt-tui` directly works too. The report is `$RPT_REPORT_DIR` when set, else the nearest folder at or above the current one with `rpt.yaml`. Outside a report, or without rpt on `PATH`, it says so and exits with code 2. To start a report, run `rpt new <dir>`.

### The screen

`1`, `2`, `3` (or `tab`) move focus between the panels, and the right side follows. **Runs** lists live runs first, then finished ones newest first, including runs from earlier sessions (dimmed). Press `?` for every key as currently bound.

### Running things

Press `:` or `ctrl-p` for the **command palette**. It lists your chains, rpt's plugins, rpt's built-in verbs as `rpt verbs` lists them (all but `add`, `new`, `plugins` and `verbs`) and rpt-tui's own actions.

![Running rpt validate, rpt set and a chain from the command palette](docs/demo/palette.gif)

- A **plugin**, or an rpt verb that takes arguments such as `set` or `delete`, opens an argument line pre-filled from its usage; `ctrl-f` inserts a file path.
- A **chain** with parameters opens a form first.
- `e` opens the selected finding, or the selected problem at its line, in your editor (`$VISUAL`, `$EDITOR`, else `nvim`, `vim`, `vi` or `nano`).
- `v` runs `rpt validate`.

Every run starts in the report folder with these environment variables:

| Variable | Holds |
|---|---|
| `RPT_REPORT_DIR` | the report folder |
| `RPT_FINDING_ID`, `RPT_FINDING_FILE` | the selected finding, when one is selected |
| `RPT_PROBLEM_FILE`, `RPT_PROBLEM_LINE` | the selected problem, when one is selected |
| `RUN_TMP` | a scratch folder, deleted when the run ends |

The selection is the highlighted row of whichever of Findings and Problems you used last.

### Working with runs

A new run gets your keys straight away, so a plugin's menu works at once.

| Key | Does |
|---|---|
| `ctrl-g` | give the keys back to rpt-tui |
| `ctrl-t` | switch between the pane and full screen (takeover) |
| `enter` | live run: give it the keys; finished run: show its full transcript |
| `f` | take over the selected run |
| `r` | run a finished run again, with the same values |
| `x` | kill a live run, or delete a finished one (asks first) |

<details>
<summary>What the marks in Runs mean</summary>

| Mark | Meaning |
|---|---|
| `● live` | running |
| `◆ quiet 12s` | running, but silent for 3 s or more (perhaps waiting for you) |
| `✔ 0` | exited 0 |
| `✘ 1`, `✘ step 2: 1` | failed, with the failing step of a chain |
| `killed` | killed by you, or on quit |
| `✘ <signal>` | ended by a signal rpt-tui didn't send |

</details>

Runs are kept between sessions. From the palette, `export-transcript` saves a run's output as plain text and `clear-history` empties the report's history.

### Notifications

A run that ends while you aren't looking at it gets a toast and a "new" dot, counted in the Runs title. A run that took 30 s or more and ended while you weren't watching (in the background, or the terminal didn't have focus) also sends a desktop notification through OSC 9 or OSC 777, whichever your terminal supports.

> [!TIP]
> Inside tmux, notifications need passthrough. Add this to `~/.tmux.conf`:
>
> ```
> set -g allow-passthrough on
> ```

## Configure

The config lives at `~/.config/rpt-tui/config.yaml` (or `$XDG_CONFIG_HOME/rpt-tui/config.yaml`). Every key is optional:

```yaml
theme: terminal          # charm (default), terminal, light or your own
keys:
  help: f1
chains:
  pdf-draft:
    description: Validate, assemble and render a draft PDF
    key: ctrl-r
    params: { CLIENT: { kind: text, default: Acme } }
    steps:
      - { run: rpt validate, allow_failure: true }
      - rpt assemble | rpt render-pdf --client "$CLIENT"
```

> [!NOTE]
> A mistake in the config never stops rpt-tui from starting: it shows what's wrong in a toast and carries on without that part. Key clashes open a guide that fixes them for you.

The config is read again when a run that edited it ends, or any time you choose `reload-config` in the palette.

**[docs/config.md](docs/config.md) covers everything you can set:** [chains](docs/config.md#chains) and their parameters, [keys](docs/config.md#keys), [themes](docs/config.md#themes), the [file picker](docs/config.md#file_picker), [history](docs/config.md#history) and [notifications](docs/config.md#notify).

## Development

```sh
cargo build --release      # target/release/rpt-tui
scripts/check.sh           # fmt, clippy, tests and cargo audit
scripts/release.sh         # on a clean tree tagged v<version>: dist/ archives and SHA256SUMS
docs/demo/setup.sh && vhs docs/demo/screen.tape   # re-record a README GIF (needs vhs and rpt)
```

> [!NOTE]
> The tests build rpt from source: clone [rpt](https://github.com/astr4liteEx/rpt) next to this repo (as `../rpt`) and install Go. `scripts/release.sh` also needs the `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` targets (`rustup target add …`).

The project's vocabulary is in [GLOSSARY.md](GLOSSARY.md), and design decisions are recorded in [docs/adr/](docs/adr/).
