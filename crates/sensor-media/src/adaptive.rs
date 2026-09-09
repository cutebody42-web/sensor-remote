//! Conservative, bounded congestion control. All times are monotonic milliseconds.
//! Tier budgets are initial tuning bounds, not claims of measured image quality.
use crate::{MediaError, VideoProfile};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const DEFAULT_MAX_BITRATE: u32 = 16_000_000;
pub const MIN_BITRATE: u32 = 300_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Telemetry {
    pub elapsed_ms: u64,
    pub captured: u64,
    pub encoded: u64,
    pub bytes: u64,
    /// Capture opportunities skipped BEFORE encoding (never discard reference frames).
    pub skipped: u64,
    pub capture_us: u64,
    pub prepare_us: u64,
    pub encode_us: u64,
    pub send_us: u64,
    pub in_flight_bytes: u64,
    pub delivery_ms: u64,
    pub profile: VideoProfile,
    pub congestion: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Feedback {
    pub delivery_ms: u64,
    pub rtt_ms: u64,
    pub decode_us: u64,
    pub prepare_encode_us: u64,
    pub write_stall_ms: u64,
    pub acknowledged: u64,
    pub saturated: bool,
}

pub struct Adaptive {
    pub profile: VideoProfile,
    bitrate: u32,
    ceiling: u32,
    baseline: u64,
    healthy: u8,
    pub congested: bool,
    fps_ceiling: u32,
}
impl Adaptive {
    pub fn new(ceiling: u32) -> Self {
        let ceiling = ceiling.clamp(MIN_BITRATE, 50_000_000);
        Self {
            profile: VideoProfile::Auto,
            bitrate: 1_800_000.min(ceiling),
            ceiling,
            baseline: u64::MAX,
            healthy: 0,
            congested: false,
            fps_ceiling: 60,
        }
    }
    pub fn bitrate(&self) -> u32 {
        self.bitrate
    }
    /// Called once per second, with observations from the preceding window.
    /// Missing feedback never grants an increase.
    pub fn observe(&mut self, f: Feedback) {
        if f.acknowledged != 0 && f.delivery_ms != 0 {
            self.baseline = self.baseline.min(f.delivery_ms);
        }
        let latency_limit = self.baseline.saturating_add(120).min(600);
        self.congested = f.saturated
            || f.write_stall_ms > 80
            || (f.delivery_ms > latency_limit && f.delivery_ms > 200);
        let work_us = f.decode_us.max(f.prepare_encode_us);
        if work_us > 25_000 {
            self.fps_ceiling = (850_000 / work_us).clamp(10, 60) as u32;
        } else if f.acknowledged != 0 && !self.congested {
            self.fps_ceiling = (self.fps_ceiling + 5).min(60);
        }
        if self.congested {
            self.bitrate = (self.bitrate * 65 / 100).max(MIN_BITRATE).min(self.ceiling);
            self.healthy = 0;
        } else if f.acknowledged != 0 && f.rtt_ms < 600 {
            self.healthy += 1;
            if self.healthy >= 3 {
                self.bitrate = (self.bitrate + (self.bitrate / 5).max(100_000)).min(self.ceiling);
                self.healthy = 0;
            }
        } else {
            self.healthy = 0;
        }
    }
    pub fn target(&self, width: u32, height: u32) -> Result<Target, MediaError> {
        crate::pixels(width, height)?;
        if width < 16 || height < 16 {
            return Err(MediaError::Dimensions);
        }
        let (w, h, fps) = match self.bitrate {
            0..800_000 => (640., 360., 15),
            800_000..1_600_000 => (960., 540., 24),
            1_600_000..3_000_000 => (1280., 720., 30),
            3_000_000..5_000_000 if self.profile == VideoProfile::Performance => (1280., 720., 60),
            3_000_000..5_000_000 if self.profile == VideoProfile::Quality => (1920., 1080., 30),
            3_000_000..5_000_000 => (1600., 900., 30),
            5_000_000..8_000_000 => (1920., 1080., 30),
            _ => (1920., 1080., 60),
        };
        let scale = (w / width as f64).min(h / height as f64).min(1.);
        Ok(Target {
            width: ((width as f64 * scale) as u32 / 2 * 2).max(16),
            height: ((height as f64 * scale) as u32 / 2 * 2).max(16),
            fps: fps.min(self.fps_ceiling),
            bitrate: self.bitrate,
        })
    }
}

/// At most 16 encoder submissions and at most 250ms of budgeted compressed data.
/// Includes asynchronous MFT input, so driver buffering cannot evade the bound.
#[derive(Default)]
pub struct FlightWindow {
    frames: VecDeque<(u64, u64, usize)>,
    bytes: usize,
}
impl FlightWindow {
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn oldest_ms(&self, now: u64) -> u64 {
        self.frames.front().map_or(0, |f| now.saturating_sub(f.1))
    }
    pub fn can_submit(&self, now: u64, bitrate: u32) -> bool {
        self.frames.len() < 16
            && self.bytes < (bitrate as usize / 32).clamp(64 * 1024, 2 * 1024 * 1024)
            && self.oldest_ms(now) < 500
    }
    pub fn submit(&mut self, sequence: u64, now: u64) {
        self.frames.push_back((sequence, now, 0));
    }
    pub fn encoded(&mut self, sequence: u64, bytes: usize) {
        if let Some(frame) = self.frames.iter_mut().find(|f| f.0 == sequence) {
            self.bytes = self.bytes.saturating_sub(frame.2).saturating_add(bytes);
            frame.2 = bytes;
        }
    }
    /// Exact, current-generation acknowledgement; duplicate/forged future ACKs
    /// cannot clear a queue. Peer remains authenticated by the session layer.
    pub fn acknowledge(&mut self, sequence: u64, now: u64) -> Option<u64> {
        let position = self.frames.iter().position(|f| f.0 == sequence)?;
        let frame = self.frames.remove(position)?;
        self.bytes = self.bytes.saturating_sub(frame.2);
        Some(now.saturating_sub(frame.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn healthy() -> Feedback {
        Feedback {
            acknowledged: 30,
            delivery_ms: 80,
            rtt_ms: 70,
            ..Feedback::default()
        }
    }
    #[test]
    fn conservative_start_slow_rise_fast_backoff_and_no_feedback_no_rise() {
        let mut a = Adaptive::new(DEFAULT_MAX_BITRATE);
        for _ in 0..10 {
            a.observe(Feedback::default());
        }
        assert_eq!(a.bitrate(), 1_800_000);
        for _ in 0..36 {
            a.observe(healthy());
        }
        assert!(a.bitrate() >= 8_000_000);
        assert_eq!(a.target(1920, 1080).unwrap().fps, 60);
        let before = a.bitrate();
        a.observe(Feedback {
            delivery_ms: 400,
            ..healthy()
        });
        assert!(a.bitrate() < before * 7 / 10);
        for _ in 0..50 {
            a.observe(Feedback {
                saturated: true,
                ..healthy()
            });
        }
        assert_eq!(a.bitrate(), MIN_BITRATE);
        assert_eq!(a.target(1920, 1080).unwrap().height, 360);
    }
    #[test]
    fn ceiling_dimensions_and_decoder_capacity_are_enforced() {
        let mut a = Adaptive::new(2_000_000);
        for _ in 0..100 {
            a.observe(healthy());
        }
        assert_eq!(a.bitrate(), 2_000_000);
        let t = a.target(640, 480).unwrap();
        assert_eq!((t.width, t.height), (640, 480));
        a.observe(Feedback {
            decode_us: 60_000,
            ..healthy()
        });
        assert!(a.target(1920, 1080).unwrap().fps <= 15);
    }
    #[test]
    fn flight_window_bounds_driver_queue_bytes_time_and_rejects_false_ack() {
        let mut w = FlightWindow::default();
        for i in 0..16 {
            assert!(w.can_submit(100, 8_000_000));
            w.submit(i, 100);
        }
        assert!(!w.can_submit(100, 8_000_000));
        assert_eq!(w.acknowledge(17, 120), None);
        assert_eq!(w.acknowledge(0, 120), Some(20));
        assert_eq!(w.acknowledge(0, 120), None);
        assert!(w.can_submit(120, 8_000_000));
        assert!(!w.can_submit(600, 8_000_000));
        w.encoded(1, 300_000);
        assert!(!w.can_submit(120, 8_000_000));
        w.acknowledge(1, 130);
        assert_eq!(w.bytes(), 0);
    }
}
