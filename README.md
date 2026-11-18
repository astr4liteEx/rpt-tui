# rpt-tui

A terminal UI for one [rpt](https://github.com/astr4liteEx/rpt) report. It shows the report's findings and problems, and runs rpt commands, plugins, your own chains of commands and your editor inside it. Each run is a real terminal, so a plugin can draw its own menus and pickers.

rpt-tui never writes report files itself: every change goes through rpt, a plugin or your editor.

Linux only (amd64 and arm64).

## Install

rpt-tui needs rpt on your `PATH`.

**From a release:** download the archive for your machine and `SHA256SUMS`, check, unpack, and put `rpt-tui` on your `PATH`:

```sh
sha256sum -c --ignore-missing SHA256SUMS
tar -xzf rpt-tui_v0.1.0_linux_amd64.tar.gz
install rpt-tui ~/.local/bin/
```

The binaries are static, so they run on any Linux.

**With cargo** (Rust 1.99 or newer):

```sh
cargo install --git https://github.com/astr4liteEx/rpt-tui --tag v0.1.0 --locked
```

This puts `rpt-tui` in `~/.cargo/bin`. For a static binary, add the musl target first and link with Rust's own linker:

```sh
rustup target add x86_64-unknown-linux-musl
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  cargo install --git https://github.com/astr4liteEx/rpt-tui --tag v0.1.0 --locked \
  --target x86_64-unknown-linux-musl
```

## Start

Inside a report folder (one holding `rpt.yaml`, or below it):

```sh
rpt tui
```

rpt finds `rpt-tui` on your `PATH` as a plugin. Running `rpt-tui` directly works too. The report is `$RPT_REPORT_DIR` when set, else the nearest folder at or above the current one with `rpt.yaml`. Outside a report, or without rpt on `PATH`, it says so and exits with code 2.

## The screen

```
 rpt-tui   ~/eng/acme-ext/report   12 findings  2 errors 1 warning
╭ 1 Findings ──────────────────────╮╭ Preview / Problem / Run ───────────────╮
│ ▌F-004 ● critical Unauth RCE …   ││                                         │
│  F-012 ● critical Kerberoast …   ││  the right side follows focus:          │
╰ 2 critical 3 high 3 medium … ────╯│   Findings → the finding                │
╭ 2 Problems ──────────────────────╮│   Problems → the problem                │
│ ✘ error Remediation is empty …   ││   Runs     → the selected run           │
╰──────────────────────────────────╯│                                         │
╭ 3 Runs · 2 new · 1 ✘ ────────────╮│                                         │
│ ● live  import-nessus SCAN=…     ││                                         │
│ ✔ 0     rpt validate   3m ago    ││                                         │
╰──────────────────────────────────╯╰─────────────────────────────────────────╯
 key hints for the focused panel
```

- **Findings** are sorted most severe first, using the report's own severity levels.
- **Problems** are what `rpt validate` reports.
- **Runs** lists live runs first, then finished ones, newest first, including runs from earlier sessions (dimmed). The right side shows the selected run.
- After any run ends, findings and problems are read again.

Press `?` for every key, as currently bound. The footer shows the keys for where you are.

## Running things

Press `:` (or `ctrl-p`) for the **command palette**. It lists your chains, rpt's plugins, rpt built-ins (`validate`, `findings`, `scope`, `assemble`, `version`, `set`, `delete`) and rpt-tui's own actions. Type to filter; `enter` chooses.

- A **plugin**, `set` or `delete` opens an argument line, pre-filled from its usage. `ctrl-f` inserts a file path.
- A **chain** with parameters opens a form first (see [Chains](docs/config.md#chains)).
- `e` opens the selected finding, or the selected problem at its line, in your editor (`$VISUAL`, `$EDITOR`, else `nvim`, `vim`, `vi` or `nano`).
- `v` runs `rpt validate`.

Every run starts in the report folder with these environment variables:

| Variable | What it holds |
|---|---|
| `RPT_REPORT_DIR` | the report folder |
| `RPT_FINDING_ID`, `RPT_FINDING_FILE` | the selected finding, when one is selected |
| `RPT_PROBLEM_FILE`, `RPT_PROBLEM_LINE` | the selected problem, when one is selected |
| `RUN_TMP` | a scratch folder, deleted when the run ends |

The selection is the highlighted row of whichever of Findings and Problems you used last.

### Runs

A new run gets your keys straight away, so a plugin's menu works at once.

| Key | What it does |
|---|---|
| `ctrl-g` | give the keys back to rpt-tui |
| `ctrl-t` | switch between the pane and full screen (takeover) |
| `enter` | on a live run: give it the keys; on a finished one: its full transcript |
| `f` | take over the selected run |
| `r` | run a finished run again, with the same values |
| `x` | kill a live run, or delete a finished one (asks first) |

The marks in Runs:

| Mark | Meaning |
|---|---|
| `● live` | running |
| `◆ quiet 12s` | running, but silent for 3 s or more (perhaps waiting for you) |
| `✔ 0` | exited 0 |
| `✘ 1`, `✘ step 2: 1` | failed, with the failing step of a chain |
| `killed` | killed by you, or on quit |
| `✘ <signal>` | ended by a signal rpt-tui didn't send |

Runs are kept between sessions (see [`history:`](docs/config.md#history)). The palette's `export-transcript` saves a run's output as plain text, and `clear-history` empties the report's history.

### When a run ends

- A run that ends while you aren't looking at it gets a toast (4 s, or 10 s for a failure) and a "new" dot, counted in the Runs title.
- A run that took 30 s or more and ended while you weren't watching sends a desktop notification. "Not watching" means it was in the background, or the terminal didn't have focus. Your terminal must support OSC 9 or OSC 777 notifications (rpt-tui picks which).
- **Inside tmux**, notifications need passthrough. Add this to `~/.tmux.conf`:

  ```
  set -g allow-passthrough on
  ```

The bell, a custom command and the 30 s threshold are set under [`notify:`](docs/config.md#notify).

## Configure

The config is `~/.config/rpt-tui/config.yaml` (or `$XDG_CONFIG_HOME/rpt-tui/config.yaml`). Every key is optional. A small one:

```yaml
theme: terminal
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

A mistake in the config never stops rpt-tui from starting: it shows what's wrong in a toast and carries on without that part. Key clashes open a guide that fixes them for you.

After editing the config in a run (`e` on a file, or your own chain), rpt-tui reads it again when the run ends. The palette's `reload-config` does it any time.

**Everything you can set is in [docs/config.md](docs/config.md):** themes, keys, chains and their parameters, the file picker, history and notifications.

## Build from source

```sh
cargo build --release      # target/release/rpt-tui
scripts/check.sh           # fmt, clippy, tests and cargo audit
scripts/release.sh         # on a clean tree tagged v<version>: dist/ archives and SHA256SUMS
```

The tests build rpt from source: clone [rpt](https://github.com/astr4liteEx/rpt) next to this repo (as `../rpt`) and install Go. `scripts/release.sh` needs the `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` targets (`rustup target add …`).
