//! Hosting runs: each child in a PTY, emulated by vt100. No ratatui here, so it is testable headless.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub mod chain;
mod encode;
mod responder;

pub use encode::{AltScreenFilter, encode_key, encode_mouse, encode_paste};
pub use responder::Colors;
use responder::Responder;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// How long `kill` waits after SIGHUP before SIGKILL.
pub const KILL_GRACE: Duration = Duration::from_millis(400);

/// Bytes of transcript a run keeps by default: the last 10 MiB.
pub const TRANSCRIPT_CAP: usize = 10 * 1024 * 1024;

/// A transcript as plain text, for export.
pub fn plain(transcript: &str) -> String {
    vt100::capture::basic_formatted_to_plain(transcript).collect()
}

/// The end of a transcript, about `max` bytes, starting at a line start when cut. The colour
/// in force where it was cut comes first, so the kept text looks as it did.
pub fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut from = text.len() - max;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let from = text[from..].find('\n').map_or(from, |i| from + i + 1);
    let mut kept = sgr_in_force(&text[..from]);
    kept.push_str(&text[from..]);
    kept
}

/// The SGR sequences (`\x1b[…m`) since the last reset in `text`: the colour in force at its end.
fn sgr_in_force(text: &str) -> String {
    let mut since_reset: Vec<&str> = vec![];
    let mut rest = text;
    while let Some(at) = rest.find("\x1b[") {
        let after = &rest[at + 2..];
        let len = after.find(|c: char| !(c.is_ascii_digit() || c == ';')).unwrap_or(after.len());
        if after[len..].starts_with('m') {
            let params = &after[..len];
            if params.is_empty() || params == "0" {
                since_reset.clear();
            } else {
                since_reset.push(&rest[at..at + 2 + len + 1]);
            }
        }
        rest = &after[len..];
    }
    since_reset.concat()
}

/// Lines of scrollback each run keeps.
const SCROLLBACK: usize = 5000;

/// The raw takeover copy of a run's output.
struct Tee {
    out: Box<dyn Write + Send>,
    filter: AltScreenFilter,
}

/// Sends `sig` to each process group. Group ids are checked positive: `killpg(0, …)` would
/// signal our own group.
fn signal_groups(groups: &[i32], sig: libc::c_int) {
    for &g in groups.iter().filter(|&&g| g > 0) {
        // SAFETY: killpg only sends a signal and touches no memory. A group that no longer
        // exists fails with ESRCH; a group id is not reused while any member lives.
        unsafe { libc::killpg(g, sig) };
    }
}

/// A PTY size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

/// What to launch: the command line, its folder, extra environment, and the colours to report.
#[derive(Debug, Clone)]
pub struct Launch {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// Variables taken out of the inherited environment.
    pub unset: Vec<String>,
    pub colors: Colors,
    /// Bytes of transcript kept: the end of it.
    pub transcript_cap: usize,
}

impl Launch {
    pub fn new(argv: Vec<String>, cwd: &Path) -> Launch {
        Launch {
            argv,
            cwd: cwd.to_path_buf(),
            env: vec![],
            unset: vec![],
            colors: Colors::default(),
            transcript_cap: TRANSCRIPT_CAP,
        }
    }
}

/// Where a run is in its life.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Live,
    Exited(u32),
    /// Ended by a signal it wasn't sent by us.
    Signalled(String),
    /// Killed by the user (or on quit).
    Killed,
}

pub struct Run {
    parser: Arc<Mutex<vt100::Parser<Responder>>>,
    responder_on: Arc<AtomicBool>,
    /// Raw takeover: where the child's bytes are copied, minus alternate-screen switches.
    tee: Arc<Mutex<Option<Tee>>>,
    /// When the child last wrote anything.
    last_output: Arc<Mutex<Instant>>,
    /// Set by the reader thread once the child's output is all read.
    drained: Arc<AtomicBool>,
    tx: Sender<Vec<u8>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    status: Status,
    /// Set by `kill`, so the end reads as killed rather than as the signal.
    killed: bool,
    /// The process groups `kill` signalled.
    kill_groups: Vec<i32>,
    size: Size,
}

impl Size {
    /// The size as vt100 wants it: at least one cell each way.
    pub(crate) fn cells(self) -> (NonZeroU16, NonZeroU16) {
        let nz = |n: u16| NonZeroU16::new(n.max(1)).unwrap_or(NonZeroU16::MIN);
        (nz(self.rows), nz(self.cols))
    }
}

fn pty_size(s: Size) -> PtySize {
    PtySize { rows: s.rows, cols: s.cols, pixel_width: 0, pixel_height: 0 }
}

fn io_err(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

impl Run {
    /// Starts `launch` in a new PTY of `size`. `dirty` is set whenever the screen changes.
    pub fn spawn(launch: Launch, size: Size, dirty: Arc<AtomicBool>) -> std::io::Result<Run> {
        let (program, args) = launch.argv.split_first().ok_or_else(|| io_err("empty command"))?;
        let pair = native_pty_system().openpty(pty_size(size)).map_err(io_err)?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.cwd(&launch.cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for k in &launch.unset {
            cmd.env_remove(k);
        }
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).map_err(io_err)?;
        // Our copy of the slave must go, or the reader never sees EOF when the child exits.
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(io_err)?;
        let mut writer = pair.master.take_writer().map_err(io_err)?;
        let responder_on = Arc::new(AtomicBool::new(true));
        let parser = Arc::new(Mutex::new({
            let (rows, cols) = size.cells();
            vt100::Parser::new_with_callbacks(
                rows,
                cols,
                SCROLLBACK,
                Responder::new(launch.colors, responder_on.clone(), launch.transcript_cap),
            )
        }));

        // Writer thread: a child that stops reading never blocks the UI.
        let (tx, rx) = channel::<Vec<u8>>();
        thread::spawn(move || {
            while let Ok(bytes) = rx.recv() {
                if writer.write_all(&bytes).and_then(|_| writer.flush()).is_err() {
                    break;
                }
            }
        });

        // Reader thread: feeds the emulator and sends the responder's answers.
        let reader_parser = parser.clone();
        let reply_tx = tx.clone();
        let drained = Arc::new(AtomicBool::new(false));
        let last_output = Arc::new(Mutex::new(Instant::now()));
        let reader_last_output = last_output.clone();
        let tee: Arc<Mutex<Option<Tee>>> = Arc::new(Mutex::new(None));
        let reader_tee = tee.clone();
        let reader_drained = drained.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                *reader_last_output.lock().unwrap() = Instant::now();
                let replies = {
                    let mut p = reader_parser.lock().unwrap();
                    p.process(&buf[..n]);
                    // Teed under the parser lock, so start_tee's repaint and the tee never interleave.
                    if let Some(t) = reader_tee.lock().unwrap().as_mut() {
                        let bytes = t.filter.filter(&buf[..n]);
                        let _ = t.out.write_all(&bytes).and_then(|_| t.out.flush());
                    }
                    std::mem::take(&mut p.callbacks_mut().replies)
                };
                if !replies.is_empty() {
                    let _ = reply_tx.send(replies);
                }
                dirty.store(true, Ordering::Release);
                // std's Mutex is unfair: without this a flood starves the UI for hundreds of ms.
                thread::yield_now();
            }
            reader_drained.store(true, Ordering::Release);
            dirty.store(true, Ordering::Release);
        });

        Ok(Run {
            parser,
            responder_on,
            tee,
            last_output,
            drained,
            tx,
            master: pair.master,
            child,
            status: Status::Live,
            killed: false,
            kill_groups: vec![],
            size,
        })
    }

    /// Checks whether the child has ended and its output is all on the screen; returns the status.
    pub fn poll(&mut self) -> Status {
        if self.status == Status::Live
            && self.drained.load(Ordering::Acquire)
            && let Ok(Some(st)) = self.child.try_wait()
        {
            self.status = match st.signal() {
                _ if self.killed => Status::Killed,
                Some(sig) => Status::Signalled(sig.to_string()),
                None => Status::Exited(st.exit_code()),
            };
        }
        self.status.clone()
    }

    /// The status as of the last `poll`.
    pub fn status(&self) -> Status {
        self.status.clone()
    }

    /// Reads the emulated screen.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        f(self.parser.lock().unwrap().screen())
    }

    /// The visible screen as plain text.
    pub fn contents(&self) -> String {
        self.parser.lock().unwrap().screen().contents()
    }

    /// Everything the run printed, coloured (vt100's basic format, rows joined by `\n`): rows
    /// evicted from scrollback, the scrollback, then the screen; the last `transcript_cap` bytes.
    pub fn transcript(&self) -> String {
        let p = self.parser.lock().unwrap();
        let r = p.callbacks();
        let (mut out, mut state) = (r.evicted.clone(), r.capture.clone());
        let range = vt100::capture::BasicFormattedCaptureRange::Full(&mut state);
        let _ = p.screen().write_contents_formatted_basic(&mut out, range);
        // The screen's empty rows below the output are no part of it.
        while let Some(nl) = out.rfind('\n')
            && plain(&out[nl + 1..]).trim().is_empty()
        {
            out.truncate(nl);
        }
        tail(&out, r.cap)
    }

    /// SIGHUP to the child's process group and the PTY's foreground group, then SIGKILL after
    /// [`KILL_GRACE`]. `Child::kill` alone reaches only the direct child.
    pub fn kill(&mut self) {
        if self.status != Status::Live {
            return;
        }
        self.killed = true;
        let mut groups: Vec<i32> = self.child.process_id().map(|p| p as i32).into_iter().collect();
        groups.extend(self.master.process_group_leader());
        groups.retain(|&g| g > 0);
        groups.dedup();
        signal_groups(&groups, libc::SIGHUP);
        let later = groups.clone();
        self.kill_groups = groups;
        thread::spawn(move || {
            thread::sleep(KILL_GRACE);
            signal_groups(&later, libc::SIGKILL);
        });
    }

    /// SIGKILL to the groups `kill` signalled, now: for quitting, when there is no "later".
    pub fn kill_now(&mut self) {
        signal_groups(&self.kill_groups, libc::SIGKILL);
    }

    /// Raw takeover: clears `out`, repaints the screen so far onto it, then copies every later
    /// byte from the child (minus alternate-screen switches). Queries go unanswered meanwhile:
    /// the real terminal answers them.
    pub fn start_tee(&mut self, mut out: Box<dyn Write + Send>) {
        let p = self.parser.lock().unwrap();
        let _ = out.write_all(b"\x1b[0m\x1b[H\x1b[2J");
        let _ = out.write_all(p.screen().state_formatted().as_bytes());
        let _ = out.flush();
        self.responder_on.store(false, Ordering::Relaxed);
        *self.tee.lock().unwrap() = Some(Tee { out, filter: AltScreenFilter::default() });
    }

    /// Ends raw takeover; the responder answers queries again.
    pub fn stop_tee(&mut self) {
        let _p = self.parser.lock().unwrap();
        *self.tee.lock().unwrap() = None;
        self.responder_on.store(true, Ordering::Relaxed);
    }

    pub fn is_teeing(&self) -> bool {
        self.tee.lock().unwrap().is_some()
    }

    /// How long since the child last wrote anything. A PTY can't say "waiting for input",
    /// so a live run that has been quiet for a while is shown as quiet.
    pub fn quiet_for(&self) -> Duration {
        self.last_output.lock().unwrap().elapsed()
    }

    /// Resizes the PTY and the emulator; the child gets SIGWINCH.
    pub fn resize(&mut self, size: Size) {
        if size == self.size || size.rows == 0 || size.cols == 0 {
            return;
        }
        self.size = size;
        let _ = self.master.resize(pty_size(size));
        let (rows, cols) = size.cells();
        self.parser.lock().unwrap().set_size(rows, cols);
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Sends a key, encoded for the child's current cursor mode.
    pub fn send_key(&self, key: &crossterm::event::KeyEvent) {
        let app_cursor = self.parser.lock().unwrap().screen().application_cursor();
        self.send(encode_key(key, app_cursor));
    }

    /// Sends a mouse event over the pane at `origin`, if the child enabled the mouse.
    pub fn send_mouse(&self, ev: &crossterm::event::MouseEvent, origin: (u16, u16)) {
        let (mode, encoding) = {
            let p = self.parser.lock().unwrap();
            (p.screen().mouse_protocol_mode(), p.screen().mouse_protocol_encoding())
        };
        self.send(encode_mouse(ev, origin, self.size, mode, encoding));
    }

    /// Sends pasted text, bracketed if the child enabled bracketed paste.
    pub fn send_paste(&self, text: &str) {
        let bracketed = self.parser.lock().unwrap().screen().bracketed_paste();
        self.send(encode_paste(text, bracketed));
    }

    /// Queues bytes for the child's input.
    pub fn send(&self, bytes: impl Into<Vec<u8>>) {
        let bytes = bytes.into();
        if !bytes.is_empty() {
            let _ = self.tx.send(bytes);
        }
    }
}
