// SPDX-License-Identifier: GPL-3.0-or-later

//! A song at one sample rate for a device at another: for the systems whose output does not
//! convert by itself.
//!
//! It draws a straight line between neighboring samples. That is cheap and keeps the pitch
//! and the time right; it lets a little of what lies above half the lower rate fold back,
//! which a windowed-sinc converter would not. A better one can take its place behind the
//! same two calls.

/// Converts stereo frames from one rate to another, a block at a time.
pub struct Resampler {
    /// Source frames for each frame put out.
    step: f64,
    /// Where the next output frame lies between `last` (0) and `next` (1).
    at: f64,
    last: [f32; 2],
    next: [f32; 2],
    /// Source frames waiting to be used, left and right in turn.
    buffer: Vec<f32>,
    used: usize,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        Resampler {
            step: f64::from(from.max(1)) / f64::from(to.max(1)),
            at: 1.0,
            last: [0.0; 2],
            next: [0.0; 2],
            buffer: Vec::with_capacity(8192),
            used: 0,
        }
    }

    /// Fills `out` (left and right in turn, at the rate converted to), asking `fill` for
    /// source frames at the rate converted from as it needs them.
    pub fn run(&mut self, out: &mut [f32], fill: &mut dyn FnMut(&mut [f32])) {
        let frames = out.len() / 2;
        for frame in out.chunks_exact_mut(2) {
            while self.at >= 1.0 {
                if self.used >= self.buffer.len() {
                    // about what a block needs, and at least a few frames
                    let wanted = (frames as f64 * self.step) as usize + 4;
                    self.buffer.resize(wanted * 2, 0.0);
                    fill(&mut self.buffer);
                    self.used = 0;
                }
                self.last = self.next;
                self.next = [self.buffer[self.used], self.buffer[self.used + 1]];
                self.used += 2;
                self.at -= 1.0;
            }
            let t = self.at as f32;
            frame[0] = self.last[0] + (self.next[0] - self.last[0]) * t;
            frame[1] = self.last[1] + (self.next[1] - self.last[1]) * t;
            self.at += self.step;
        }
    }
}
