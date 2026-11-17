# rpt-tui

A terminal UI that runs inside one rpt report and launches rpt, its plugins and user-defined chains. Report, finding, plugin and core mean what `../rpt/GLOSSARY.md` says.

**Command**:
Anything the TUI can launch: an rpt built-in verb, a plugin, a chain, or the editor.
_Avoid_: action, task, job

**Chain**:
A command the user defines in the TUI config, made of steps that run rpt, plugins or other programs.
_Avoid_: macro, script, pipeline (a chain may contain pipelines, but is not one)

**Step**:
One command line of a chain; a chain's steps run in order within a single run and stop at the first that fails.
_Avoid_: stage, task

**Parameter**:
A value a chain asks for each time it is run, such as the scan file to import, handed to its steps.
_Avoid_: variable, input (that is a plugin's declared data type)

**Selection**:
The finding or problem currently highlighted in the TUI, which every run can read and which a chain can require.
_Avoid_: current item, focus (that is which panel receives keys)

**Hotkey**:
A key the user binds to a chain in the config, which runs that chain while keys go to the TUI.
_Avoid_: shortcut, binding (alone)

**Theme**:
The named set of colours the TUI draws with, including a colour for each severity level; built in or a user file, chosen in the config.
_Avoid_: colour scheme, skin, palette (that is the command palette)

**Action**:
A named built-in thing the TUI does on a key, such as opening the palette or leaving a run; the user can rebind or unbind it.
_Avoid_: command (that is something the TUI launches as a run), shortcut

**File picker**:
The command the TUI runs to choose a file for a parameter, by default fzf; its output becomes the value.
_Avoid_: browser, file dialog

**Run**:
One execution of a command: its live terminal, its transcript, exit code and timing.
_Avoid_: job, session, process

**Run history**:
The finished runs the TUI keeps for a report across sessions, outside the report folder.
_Avoid_: log, job list

**Transcript**:
Everything a run printed, kept with its colours so a finished run can be reopened and read.
_Avoid_: log, output file

**Ephemeral run**:
A run that leaves no trace in run history once it ends, for commands run often and only to look at something.
_Avoid_: temporary run, unlogged run

**Pane**:
Showing a run's terminal inside the TUI, with the TUI's own chrome still visible.
_Avoid_: embedded view, window

**Takeover**:
Showing a run's terminal by suspending the TUI and giving the run the whole terminal until it ends or is sent back to a pane.
_Avoid_: fullscreen mode, suspend (that is the mechanism, not the mode)
