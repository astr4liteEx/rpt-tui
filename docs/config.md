# Configuring rpt-tui

The config is one YAML file:

- `$XDG_CONFIG_HOME/rpt-tui/config.yaml`, or
- `~/.config/rpt-tui/config.yaml` when `XDG_CONFIG_HOME` isn't set.

Every key is optional, and a missing file means the defaults.

```yaml
theme: charm                      # or { base, colors, severity, severity_scale }
file_picker: "fzf"                # default: fzf if on PATH, else the built-in list
keys:                             # action: key | [keys] | null
  leave_run: ctrl-g
history: { dir: ~/.local/state/rpt-tui, max_runs: 200, max_days: 30, max_transcript_mib: 10 }
notify:  { desktop: true, bell: false, after_secs: 30, command: null }
chains:
  import-nessus:
    description: Parse a Nessus scan, match scope, add findings
    key: ctrl-n
    params:
      SCAN:   { kind: file, glob: "*.nessus", remember: true }
      STRICT: { kind: choice, options: [yes, no], default: no }
    steps:
      - rpt parse-nessus "$SCAN" > "$RUN_TMP/records.jsonl"
      - rpt scope match < "$RUN_TMP/records.jsonl" > "$RUN_TMP/scoped.jsonl"
      - rpt add < "$RUN_TMP/scoped.jsonl"
```

**Mistakes:** a mistake never stops rpt-tui from starting. It shows what's wrong in a toast and uses the default for that setting. A broken chain is left out of the palette until you fix it.

**Booleans:** only `true` and `false` are booleans (YAML 1.2). `yes` and `no` are ordinary words, which is what a `choice` like the one above wants.

**Reloading:** rpt-tui reads the config, and your theme files, again in two cases:

- when a run ends and one of them has changed since it was read (after you edit `config.yaml` with `e`, say);
- when you choose `reload-config` in the palette.

It never watches the files. A reload brings in the theme, chains, keys, `file_picker` and `notify`, and lists rpt's plugins again. Live runs are left alone. A change under `history:` waits for the next start, and a toast says so.

**Edits rpt-tui makes:** rpt-tui changes `config.yaml` itself in only two places, the theme picker and the key guide. Each edit touches only the line it needs and keeps your comments. Before its first edit, it copies the file to `config.yaml.bak`.

If the file is written in a way it can't safely edit, it leaves the file alone. The change then lasts the session, and a toast gives you the line to add yourself.

- [Chains](#chains)
- [Keys](#keys)
- [Themes](#themes)
- [The file picker](#file_picker)
- [History](#history)
- [Notifications](#notify)

## Chains

`chains:` holds your own commands, keyed by name, in the order the palette lists them.

- A name is lowercase letters, digits and hyphens.
- It can't be an rpt built-in verb (`add` included) or a plugin's name.
- A chain named like a plugin is found once plugins are listed. It is then left out, with a toast.

| Key | Value | Default |
|---|---|---|
| `steps` | **required**: the shell lines to run, in order | |
| `description` | shown in the palette and in help | none |
| `key` | a hotkey that runs it from anywhere (see [Keys](#keys)) | none |
| `params` | values to ask for before it runs (see below) | none |
| `needs` | `finding`, `problem` or `any`: the chain only runs with that selected | none |
| `ephemeral` | `true`: the run disappears when it ends and isn't kept in history | `false` |
| `display` | `pane` or `takeover` (full screen) | `pane` |
| `takeover` | `emulated` or `raw`: how it takes over (see below) | `emulated` |
| `notify` | `true`: always notify when it ends; `false`: never | the [`notify:`](#notify) rule |

### Steps

Each step is a shell line run by `bash -o pipefail -c`, written either way:

```yaml
steps:
  - rpt assemble > "$RUN_TMP/out.md"
  - { run: rpt validate, allow_failure: true }
```

- **One run:** all steps run in one terminal, one after another, in the report folder.
- **Headers:** with two or more steps, each starts with a header line like `── step 2/3: <line>`.
- **Failure:** the chain stops at the first step that exits non-zero, and its run is marked `✘ step 2: 1`. A step with `allow_failure: true` prints `── step 2/3 failed (1), continuing` instead.
- **`rpt`:** `rpt` in a step is the rpt that rpt-tui itself uses.

Every run gets these environment variables:

| Variable | What it holds |
|---|---|
| `RPT_REPORT_DIR` | the report folder |
| `RPT_FINDING_ID`, `RPT_FINDING_FILE` | the selected finding (the file path absolute) |
| `RPT_PROBLEM_FILE`, `RPT_PROBLEM_LINE` | the selected problem (no line when it has none) |
| `RUN_TMP` | a scratch folder for this run, deleted when it ends |
| each parameter | its value, by its name |

Only the selected kind's variables are set: a selected finding sets no problem variables, and the other way round.

### Parameters

```yaml
params:
  SCAN:    { kind: file, glob: "*.nessus", remember: true }
  STRICT:  { kind: choice, options: [yes, no], default: no }
  CLIENT:  { kind: text, default: Acme }
  VERBOSE: { kind: flag }
  NOTE:    { kind: text, optional: true }
```

- A parameter reaches the steps as an environment variable of the same name. Quote it in the shell, as in `"$SCAN"`.
- A name is letters, digits and `_`, not starting with a digit.

| Key | Value |
|---|---|
| `kind` | `text`, `file`, `choice` or `flag` |
| `glob` | `file` only: which files the picker offers. Without `/` it matches file names at any depth; with `/`, the path from the report folder |
| `options` | `choice` only: the values to choose from |
| `default` | the starting value. A `choice` default must be one of its options; a `flag` default is `true`/`false` or `yes`/`no` |
| `optional` | `true`: may be left empty (it is then set to the empty string) |
| `remember` | `true`: start from the last value you used in this report |

- **Required:** a parameter is required unless it has a `default` or is `optional`.
- **Flags:** a `flag` is `yes` or `no`, and starts at `no`.
- **The form:** it opens before the chain runs; a chain with no parameters runs at once. Its keys:

  | Key | What it does |
  |---|---|
  | `tab` / `down`, `shift-tab` / `up` | move between fields |
  | `left` / `right` | cycle a choice |
  | `space` | toggle a flag |
  | `ctrl-f` | pick a file |
  | `enter` | run |
  | `esc` | go back to the palette |

### Pane, takeover and raw

- **Where it opens:** a run opens in the pane on the right, or full screen with `display: takeover`. `ctrl-t` switches between the two at any time.
- **`takeover: emulated`** (the default): rpt-tui draws the run itself.
- **`takeover: raw`**: hands your real terminal to the program, which suits editors and full-screen tools. `ctrl-t` comes back.

### Replacing `edit`

`e` runs the built-in `edit` chain. It opens the selected finding, or the selected problem at its line, full screen in your editor.

A chain of your own named `edit` replaces it. For example, nvim in the pane:

```yaml
chains:
  edit:
    description: Edit in a pane
    needs: any
    steps: ['nvim "${RPT_PROBLEM_FILE:-$RPT_FINDING_FILE}"']
```

Your `edit` inherits nothing from the built-in, so write `needs:` yourself.

## Keys

`keys:` binds an action to one key, a list of keys, or `null` to unbind it:

```yaml
keys:
  palette: [":", ctrl-k]
  help: f1
  validate: null
```

### How keys are written

These rules are the same for `keys:` and a chain's `key:`.

- **Characters** are case-sensitive: `G` is shift-g.
- **Named keys:**
  - `enter`, `esc`, `tab`, `space`, `backspace`;
  - `up`, `down`, `left`, `right`;
  - `home`, `end`, `pgup`, `pgdn`;
  - `f1`–`f12`.
- **Modifiers** join with `-`: `ctrl-n`, `alt-i`, `ctrl-alt-x`.

### Actions and their default keys

| Where | Action | Default |
|---|---|---|
| anywhere | `palette` | `:` `ctrl-p` |
| | `quit` (asks first if runs are live) | `q` `ctrl-c` |
| | `help` | `?` |
| | `focus_next`, `focus_findings`, `focus_problems`, `focus_runs` | `tab`, `1`, `2`, `3` |
| | `validate` | `v` |
| | `theme`, `reload-config`, `clear-history`, `export-transcript`, `fix-keys` | unbound (in the palette) |
| lists | `down`, `up` | `j` `down`, `k` `up` |
| | `top`, `bottom` | `g`, `G` |
| | `open` (full view, or the keys to a run), `back` | `enter`, `esc` |
| Findings, Problems | `edit` | `e` |
| Runs | `rerun`, `kill_or_delete`, `takeover` | `r`, `x`, `f` |
| a run with the keys | `leave_run`, `toggle_takeover` | `ctrl-g`, `ctrl-t` |

- While a run has the keys, only `leave_run` and `toggle_takeover` are caught; every other key goes to the run.
- `toggle_takeover` must be a single byte (a character or a `ctrl-` key), since raw takeover reads raw bytes.
- The keys inside the palette, the form and the pickers can't be changed.

### Clashes

**What clashes:**

- A chain's hotkey works anywhere, so it can't use a key that any action or panel uses.
- Two actions in the same place can't share a key.

**At startup:** clashes and keys rpt-tui can't read open a guide before anything else. It shows one problem at a time, and you rebind or unbind either side. The fix is saved to `config.yaml`.

- **Skipping:** `esc` skips a problem. For the session, the action then keeps its default key and the chain loses its hotkey.
- **Later:** `fix-keys` in the palette reopens the guide.

`?` lists every key as currently bound, with your changes marked `*`.

## Themes

```yaml
theme: light
```

### Built-in themes

| Theme | For |
|---|---|
| `charm` (default) | dark terminals with true colour |
| `terminal` | your terminal's own 16 colours and background |
| `light` | light backgrounds |

**Switching:** `theme` in the palette opens a picker that previews each theme live. `enter` keeps it and writes `theme:` to your config; `esc` goes back.

### Your own theme

`theme:` can also be a theme built on another, with your changes on top:

```yaml
theme:
  base: terminal
  colors:
    accent: "#ff79c6"
    border_run: magenta
  severity:
    "*crit*": bright-red
    p1: red
  severity_scale: [red, yellow, blue]
```

**Theme files:** a theme of the same shape can go in its own file, `themes/<name>.yaml` beside `config.yaml`, and then `theme: <name>` uses it. A theme file can build on another theme file through `base:`. Built-in names are taken: a `themes/charm.yaml` is ignored, with a toast.

| Key | Value |
|---|---|
| `base` | the theme it builds on: a built-in, or a theme file's name. Default `charm` |
| `colors` | interface colours, by token (below) |
| `severity` | colours for severity levels, by name or by a glob with `*` and `?` |
| `severity_scale` | colours for levels by rank, most severe first; the last repeats |

**Tokens:**

| Token | Colours |
|---|---|
| `bg` | the background |
| `surface` | popups and toasts |
| `text` | text |
| `muted` | secondary text |
| `border` | a panel's border |
| `border_focus` | the focused panel's border |
| `border_run` | a run pane's border while the run has the keys |
| `accent` | highlights |
| `ok` | success |
| `warn` | warnings |
| `err` | failures |

**Severity colours:** each level's colour is the first of these that matches:

1. **Your theme's own `severity:` names**, ignoring case.
2. **Your theme's own globs**, in the order written.
3. **The base theme's names and globs**, the same way.
4. **The level's rank** in the report (`rpt severities`), read from `severity_scale`.

So `"*crit*": bright-red` colours `critical` even though the base theme names it. The built-in themes name `critical`, `high`, `medium`, `low` and `info` as the first to fifth places on the scale, so a new `severity_scale` recolours them too.

**Colours** are written as:

- `#rrggbb`;
- one of the 16 names: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, and `bright-` before each (any case);
- an index `0`–`255`;
- `default`, the terminal's own colour.

**Without true colour:** when `COLORTERM` isn't `truecolor` or `24bit`, `#rrggbb` colours are drawn as the nearest of the 256-colour palette's fixed colours (16–255), never your terminal's own 16.

**Programs in runs:** they keep their own colours. Their default foreground and background are the theme's `text` and `bg`, and that's what they're told if they ask.

## `file_picker`

- **What it does:** it chooses a file for a `file` parameter, and for `ctrl-f` in the palette's argument line.
- **The default:** `fzf` when it's on your `PATH`, else a built-in list with a filter line.
- **Your own command:**

  ```yaml
  file_picker: "fzf --height 100% --preview 'bat --color=always {}'"
  ```

  The command gets the report's matching files on stdin. When it exits 0, the first line it prints is the value.

## `history`

Runs are kept between sessions, per report, outside the report folder.

```yaml
history:
  dir: ~/.local/state/rpt-tui
  max_runs: 200
  max_days: 30
  max_transcript_mib: 10
```

| Key | Value | Default |
|---|---|---|
| `dir` | where history goes; a leading `~/` is your home | `$XDG_STATE_HOME/rpt-tui`, else `~/.local/state/rpt-tui` |
| `max_runs` | runs kept per report | `200` |
| `max_days` | days a run is kept | `30` |
| `max_transcript_mib` | the last this many MiB of each run's output are kept | `10` |

- **What's kept:** each run's command, values, times, how it ended and its coloured output. `remember: true` parameters and the palette's "recently used" are stored here too.
- **Pruning:** at startup and after each run is saved, history keeps the newest `max_runs` runs that are younger than `max_days`.
- **Without a home:** with neither `HOME` nor `XDG_STATE_HOME` set, history is off, and a toast says so.

## `notify`

Says when a run ends while you weren't watching it.

```yaml
notify:
  desktop: true
  bell: false
  after_secs: 30
  command: null
```

| Key | Value | Default |
|---|---|---|
| `desktop` | a desktop notification through the terminal (OSC 9, or OSC 777 where that's the one that works) | `true` |
| `bell` | ring the terminal bell | `false` |
| `after_secs` | only runs that lasted at least this long notify | `30` |
| `command` | run this instead of the desktop notification | none |

**Unwatched:** a run counts as unwatched if it wasn't on screen when it ended, or the terminal didn't have focus. A run you kill yourself never notifies. A chain's own `notify: true` or `notify: false` overrides `after_secs` and watching.

**Inside tmux:** notifications need `set -g allow-passthrough on` in `~/.tmux.conf`.

**The command:**

- **How it runs:** through `bash -c` in the background, never as a run. Its output is thrown away; if it fails, a toast says so.
- **What it gets:**

  | Variable | Value |
  |---|---|
  | `RUN_NAME` | the run's label, e.g. `import-nessus SCAN=scans/a.nessus` |
  | `RUN_STATUS` | `ok` or `failed` |
  | `RUN_EXIT` | the exit code, or 128 + the signal number |
  | `RUN_DURATION` | whole seconds |
  | `RPT_REPORT_DIR` | the report folder |

For example:

```yaml
notify:
  after_secs: 60
  command: 'notify-send "rpt: $RUN_NAME" "$RUN_STATUS in ${RUN_DURATION}s"'
```

**Toasts and dots:** these show whatever `notify:` says. A run that ends while you aren't looking at it always gets a toast and a "new" dot in Runs.
