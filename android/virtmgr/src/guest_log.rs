// Copyright (C) 2026 DiamaneOS
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! DiamaneOS: a guest's console and log output, read for logcat when the VM's client takes neither.
//!
//! The guest decides how long a line is and how many it writes. The reader holds one fixed read buffer and at most
//! `MAX_RECORD` bytes of the current line, so a line without an end costs no more memory than a short one. The rest
//! of a longer line is dropped and counted. Lines go to the log at most `RATE_PER_SECOND` per second for each VM,
//! after a burst of `BURST`, shared by the VM's console and log. Lines over the rate are dropped and counted, and the
//! counts are logged once lines pass again.

use std::io::{ErrorKind, Read};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Output chosen before creating any manager reader, shared by console and log.
pub fn output<T, R, E, P>(
    supplied: Option<T>,
    debuggable: Result<bool, E>,
    pipe: impl FnOnce() -> Result<(T, R), P>,
) -> Result<(Option<T>, Option<R>), P> {
    if let Some(fd) = supplied {
        return Ok((Some(fd), None));
    }
    if !debuggable.unwrap_or(false) {
        return Ok((None, None));
    }
    let (write, read) = pipe()?;
    Ok((Some(write), Some(read)))
}

/// The crosvm output argument for the chosen descriptor, or its sink.
pub fn output_arg(path: Option<String>) -> String {
    path.map_or_else(|| "type=sink".to_string(), |path| format!("type=file,path={path}"))
}

/// The most bytes of one line that reach the log.
pub const MAX_RECORD: usize = 1024;
/// The read buffer.
const READ_CHUNK: usize = 4096;
/// Lines one VM may log at once, and how many more each second.
pub const BURST: u32 = 200;
/// Lines one VM may log each second once its burst is used.
pub const RATE_PER_SECOND: u32 = 50;

/// The lines one VM may still log, refilled with time. One for each VM, shared by its readers.
pub struct Rate {
    state: Mutex<RateState>,
}

struct RateState {
    tokens: u32,
    last: Instant,
    /// Nanoseconds towards the next token.
    carry: u128,
    dropped: u64,
}

/// What a line may do: be logged (after a count of the lines dropped before it, if any), or be dropped.
#[derive(Debug, PartialEq, Eq)]
pub enum Pass {
    Log { dropped_before: u64 },
    Drop,
}

impl Rate {
    pub fn new(now: Instant) -> Self {
        Rate { state: Mutex::new(RateState { tokens: BURST, last: now, carry: 0, dropped: 0 }) }
    }

    /// Takes one line's token at `now`.
    pub fn take(&self, now: Instant) -> Pass {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let elapsed = now.saturating_duration_since(s.last).as_nanos() + s.carry;
        let per_token = Duration::from_secs(1).as_nanos() / RATE_PER_SECOND as u128;
        let new = elapsed / per_token;
        s.carry = elapsed % per_token;
        s.last = s.last.max(now);
        s.tokens = (s.tokens as u128 + new).min(BURST as u128) as u32;
        if s.tokens == 0 {
            s.dropped += 1;
            return Pass::Drop;
        }
        s.tokens -= 1;
        Pass::Log { dropped_before: std::mem::take(&mut s.dropped) }
    }

    /// Lines dropped since the last logged line.
    #[cfg(test)]
    pub fn dropped(&self) -> u64 {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).dropped
    }
}

/// Reads `from` until its end and hands each line to `emit`, at most `MAX_RECORD` bytes of it, with the count of its
/// bytes left out. Lines over `rate` are not handed on: the next line that passes says how many. `now` is the clock.
pub fn pump(
    mut from: impl Read,
    rate: &Rate,
    now: impl Fn() -> Instant,
    mut emit: impl FnMut(String),
) -> std::io::Result<()> {
    let mut chunk = [0u8; READ_CHUNK];
    let mut line: Vec<u8> = Vec::with_capacity(MAX_RECORD);
    let mut cut: usize = 0;
    let mut finish = |line: &mut Vec<u8>, cut: &mut usize| {
        if line.last() == Some(&b'\r') && *cut == 0 {
            // Logs sent via TTY usually end lines with "\r\n".
            line.pop();
        }
        match rate.take(now()) {
            Pass::Log { dropped_before } => {
                if dropped_before > 0 {
                    emit(format!(
                        "({dropped_before} lines dropped, over {RATE_PER_SECOND} lines a second)"
                    ));
                }
                let mut text = String::from_utf8_lossy(line).into_owned();
                if *cut > 0 {
                    text.push_str(&format!(" ({} more bytes dropped)", *cut));
                }
                emit(text);
            }
            Pass::Drop => {}
        }
        line.clear();
        *cut = 0;
    };
    loop {
        let n = match from.read(&mut chunk) {
            Ok(0) => {
                if !line.is_empty() || cut > 0 {
                    finish(&mut line, &mut cut);
                }
                return Ok(());
            }
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        for piece in chunk[..n].split_inclusive(|b| *b == b'\n') {
            let (body, ends) = match piece.split_last() {
                Some((b'\n', body)) => (body, true),
                _ => (piece, false),
            };
            let room = MAX_RECORD - line.len();
            let keep = body.len().min(room);
            line.extend_from_slice(&body[..keep]);
            cut += body.len() - keep;
            if ends {
                finish(&mut line, &mut cut);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A reader that hands out `total` bytes of `byte`, a newline after every `line` bytes, in reads of `step`.
    struct Lines {
        left: usize,
        line: usize,
        at: usize,
        step: usize,
        byte: u8,
    }

    impl Read for Lines {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.left).min(self.step);
            for b in &mut buf[..n] {
                self.at += 1;
                *b = if self.at.is_multiple_of(self.line + 1) { b'\n' } else { self.byte };
            }
            self.left -= n;
            Ok(n)
        }
    }

    fn lines(total: usize, line: usize, step: usize) -> Lines {
        Lines { left: total, line, at: 0, step, byte: b'a' }
    }

    #[test]
    fn a_line_without_end_is_cut_at_the_record_size() {
        let rate = Rate::new(Instant::now());
        let mut out = vec![];
        // 64 MiB without a newline: the guest decides, the reader holds one record.
        pump(lines(64 << 20, usize::MAX - 1, 65536), &rate, Instant::now, |s| out.push(s)).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with(&"a".repeat(MAX_RECORD)));
        assert_eq!(
            out[0],
            format!("{} ({} more bytes dropped)", "a".repeat(MAX_RECORD), (64 << 20) - MAX_RECORD)
        );
    }

    #[test]
    fn every_record_is_bounded() {
        let rate = Rate::new(Instant::now());
        let mut longest = 0;
        let mut count = 0;
        pump(lines(1 << 20, 10_000, 1000), &rate, Instant::now, |s| {
            longest = longest.max(s.len());
            count += 1;
        })
        .unwrap();
        assert!(count > 0);
        assert!(longest <= MAX_RECORD + 64, "a record of {longest} bytes");
    }

    #[test]
    fn short_lines_are_rate_limited_per_vm() {
        // A frozen clock: the burst passes, then nothing, and two readers of one VM share the budget.
        let start = Instant::now();
        let rate = Rate::new(start);
        let mut out = vec![];
        pump(lines(100_000, 9, 4096), &rate, || start, |s| out.push(s)).unwrap();
        pump(lines(100_000, 9, 4096), &rate, || start, |s| out.push(s)).unwrap();
        assert_eq!(out.len(), BURST as usize);
        assert_eq!(rate.dropped(), 2 * 10_000 - BURST as u64);
    }

    #[test]
    fn dropped_lines_are_counted_once_lines_pass_again() {
        let start = Instant::now();
        let rate = Rate::new(start);
        let t = Cell::new(start);
        let mut out = vec![];
        pump(lines(10 * (BURST as usize + 5), 9, 4096), &rate, || t.get(), |s| out.push(s))
            .unwrap();
        assert_eq!(out.len(), BURST as usize);
        t.set(start + Duration::from_secs(1));
        pump(lines(10, 9, 4096), &rate, || t.get(), |s| out.push(s)).unwrap();
        assert_eq!(
            out[BURST as usize],
            format!("(5 lines dropped, over {RATE_PER_SECOND} lines a second)")
        );
        assert_eq!(out[BURST as usize + 1], "a".repeat(9));
    }

    #[test]
    fn the_rate_refills_to_its_burst_only() {
        let start = Instant::now();
        let rate = Rate::new(start);
        for _ in 0..BURST {
            assert_eq!(rate.take(start), Pass::Log { dropped_before: 0 });
        }
        assert_eq!(rate.take(start), Pass::Drop);
        // A tenth of a second gives five lines at 50 a second.
        let later = start + Duration::from_millis(100);
        for _ in 0..5 {
            assert!(matches!(rate.take(later), Pass::Log { .. }));
        }
        assert_eq!(rate.take(later), Pass::Drop);
        // An hour later: the burst, not more.
        let much_later = start + Duration::from_secs(3600);
        let passed =
            (0..BURST + 10).filter(|_| matches!(rate.take(much_later), Pass::Log { .. })).count();
        assert_eq!(passed, BURST as usize);
    }

    #[test]
    fn output_preserves_the_callers_pipe_error_type() {
        #[derive(Debug, PartialEq)]
        enum PipeError {
            Exhausted,
        }
        let result =
            output(None::<u32>, Ok::<_, ()>(true), || Err::<(u32, u32), _>(PipeError::Exhausted));
        assert_eq!(result, Err(PipeError::Exhausted));
    }

    #[test]
    fn production_streams_are_sinks_unless_a_client_supplies_an_fd() {
        for stream in ["console", "log"] {
            for property in [Ok(false), Err(())] {
                let (fd, reader) =
                    output(None::<String>, property, || -> std::io::Result<(String, ())> {
                        panic!("{stream}: a user build must create no reader")
                    })
                    .unwrap();
                assert!(reader.is_none());
                assert_eq!(output_arg(fd), "type=sink");
            }
            for property in [Ok(false), Ok(true), Err(())] {
                let (fd, reader) = output(
                    Some("/proc/self/fd/7".to_string()),
                    property,
                    || -> std::io::Result<(String, ())> { panic!("supplied FD needs no reader") },
                )
                .unwrap();
                assert!(reader.is_none());
                assert_eq!(output_arg(fd), "type=file,path=/proc/self/fd/7");
            }
            let (fd, reader) =
                output(None::<String>, Ok::<_, ()>(true), || -> std::io::Result<(String, i32)> {
                    Ok(("/proc/self/fd/8".to_string(), 8))
                })
                .unwrap();
            assert_eq!(reader, Some(8));
            assert_eq!(output_arg(fd), "type=file,path=/proc/self/fd/8");
        }
    }

    #[test]
    fn delayed_reader_does_not_refill_an_interval_twice() {
        let start = Instant::now();
        let rate = Rate::new(start);
        for _ in 0..BURST {
            rate.take(start);
        }
        let later = start + Duration::from_millis(100);
        for _ in 0..5 {
            assert!(matches!(rate.take(later), Pass::Log { .. }));
        }
        assert_eq!(rate.take(start), Pass::Drop);
        assert_eq!(rate.take(later), Pass::Drop);
    }

    #[test]
    fn lines_keep_their_text() {
        let rate = Rate::new(Instant::now());
        let mut out = vec![];
        pump(&b"one\r\ntwo\nthree"[..], &rate, Instant::now, |s| out.push(s)).unwrap();
        assert_eq!(out, ["one", "two", "three"]);
    }
}
