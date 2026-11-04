//! Input-to-paint timing, on when `DRUCK_TRACE` is set in a debug build or a build with the `measure` feature. Prints one line to stderr for each input that reached a frame.

use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Trace {
    enabled: bool,
    input: Option<Instant>,
    edit: Duration,
    prepaint_started: Option<Instant>,
    paint_started: Option<Instant>,
    prepaint: Duration,
}

impl Trace {
    pub fn from_env() -> Self {
        Self {
            enabled: crate::gate::var("DRUCK_TRACE").is_some(),
            ..Self::default()
        }
    }

    /// An input arrived. The first one since the last frame sets the start of the measurement.
    pub fn input(&mut self) -> Instant {
        let now = Instant::now();
        if self.enabled && self.input.is_none() {
            self.input = Some(now);
        }
        now
    }

    /// The input's handler finished, having started at `started`.
    pub fn handled(&mut self, started: Instant) {
        self.edit += started.elapsed();
    }

    pub fn prepaint_started(&mut self) {
        if self.enabled {
            self.prepaint_started = Some(Instant::now());
        }
    }

    pub fn paint_started(&mut self) {
        if let Some(started) = self.prepaint_started.take() {
            self.prepaint = started.elapsed();
            self.paint_started = Some(Instant::now());
        }
    }

    pub fn painted(&mut self) {
        let (Some(paint), Some(input)) = (self.paint_started.take(), self.input.take()) else {
            return;
        };
        eprintln!(
            "druck-trace input_to_paint_us={} edit_us={} prepaint_us={} paint_us={}",
            input.elapsed().as_micros(),
            self.edit.as_micros(),
            self.prepaint.as_micros(),
            paint.elapsed().as_micros(),
        );
        self.edit = Duration::ZERO;
    }
}
