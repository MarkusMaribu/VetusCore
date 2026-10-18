/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Per-environment game time. Host UI, audio and diagnostics keep real time.
//! Rebase at each speed change so guest clocks never jump backwards.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

struct Pause {
    accumulated: Duration,
    frozen_since: Option<Instant>,
}

static PAUSE: Mutex<Pause> = Mutex::new(Pause {
    accumulated: Duration::ZERO,
    frozen_since: None,
});

/// Freeze every guest clock. Unfreezing rebases host time so paused seconds
/// are not delivered as a burst of guest timers.
pub fn set_frozen(frozen: bool) {
    let mut pause = PAUSE.lock().unwrap();
    if frozen {
        if pause.frozen_since.is_none() {
            pause.frozen_since = Some(Instant::now());
        }
    } else if let Some(start) = pause.frozen_since.take() {
        pause.accumulated += Instant::now().saturating_duration_since(start);
    }
}

fn effective_now() -> Instant {
    let now = Instant::now();
    let pause = PAUSE.lock().unwrap();
    let frozen = pause
        .frozen_since
        .map(|start| now.saturating_duration_since(start))
        .unwrap_or_default();
    now.checked_sub(pause.accumulated + frozen).unwrap_or(now)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Speed {
    Quarter,
    Half,
    #[default]
    Normal,
    Double,
    Quadruple,
}

impl Speed {
    const ALL: [Self; 5] = [Self::Quarter, Self::Half, Self::Normal, Self::Double, Self::Quadruple];

    pub fn multiplier(self) -> f64 {
        match self {
            Self::Quarter => 0.25,
            Self::Half => 0.5,
            Self::Normal => 1.0,
            Self::Double => 2.0,
            Self::Quadruple => 4.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Quarter => "0.25x",
            Self::Half => "0.5x",
            Self::Normal => "1x",
            Self::Double => "2x",
            Self::Quadruple => "4x",
        }
    }

    pub fn step(self, faster: bool) -> Self {
        let i = Self::ALL.iter().position(|&s| s == self).unwrap();
        Self::ALL[if faster { (i + 1).min(4) } else { i.saturating_sub(1) }]
    }
}

pub struct GuestClock {
    host_anchor: Instant,
    guest_anchor: Instant,
    wall_anchor: SystemTime,
    speed: Speed,
}

impl Default for GuestClock {
    fn default() -> Self {
        Self::new()
    }
}

impl GuestClock {
    pub fn new() -> Self {
        Self::at(Instant::now(), SystemTime::now())
    }

    fn at(host: Instant, wall: SystemTime) -> Self {
        Self { host_anchor: host, guest_anchor: host, wall_anchor: wall, speed: Speed::Normal }
    }

    fn elapsed_at(&self, host: Instant) -> Duration {
        host.saturating_duration_since(self.host_anchor).mul_f64(self.speed.multiplier())
    }

    pub fn now(&self) -> Instant {
        self.guest_anchor + self.elapsed_at(effective_now())
    }

    pub fn system_time(&self) -> SystemTime {
        self.wall_anchor + self.elapsed_at(effective_now())
    }

    pub fn speed(&self) -> Speed {
        self.speed
    }

    pub fn set_speed(&mut self, speed: Speed) {
        self.set_speed_at(speed, effective_now());
    }

    fn set_speed_at(&mut self, speed: Speed, host: Instant) {
        let elapsed = self.elapsed_at(host);
        self.guest_anchor += elapsed;
        self.wall_anchor += elapsed;
        self.host_anchor = host;
        self.speed = speed;
    }

    /// Convert a guest deadline to host time before comparing with audio,
    /// composition or scheduler deadlines. Recompute after a speed change.
    pub fn host_deadline(&self, guest: Instant) -> Instant {
        self.host_deadline_at(guest, effective_now())
    }

    fn host_deadline_at(&self, guest: Instant, host: Instant) -> Instant {
        let remaining = guest.saturating_duration_since(self.guest_anchor + self.elapsed_at(host));
        host + remaining.div_f64(self.speed.multiplier())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_and_speed_changes_preserve_continuity() {
        let start = Instant::now();
        let wall = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let mut clock = GuestClock::at(start, wall);
        for speed in Speed::ALL {
            clock.set_speed_at(speed, start);
            assert_eq!(clock.guest_anchor, start);
            assert_eq!(clock.wall_anchor, wall);
            assert_eq!(clock.elapsed_at(start + Duration::from_secs(4)),
                       Duration::from_secs_f64(4.0 * speed.multiplier()));
        }
        clock.set_speed_at(Speed::Double, start);
        clock.set_speed_at(Speed::Half, start + Duration::from_secs(10));
        assert_eq!(clock.guest_anchor, start + Duration::from_secs(20));
        assert_eq!(clock.wall_anchor, wall + Duration::from_secs(20));
        clock.set_speed_at(Speed::Normal, start + Duration::from_secs(14));
        assert_eq!(clock.guest_anchor, start + Duration::from_secs(22));
        assert_eq!(clock.wall_anchor, wall + Duration::from_secs(22));
    }

    #[test]
    fn pending_deadlines_follow_new_speed() {
        let start = Instant::now();
        let mut clock = GuestClock::at(start, SystemTime::UNIX_EPOCH);
        let due = start + Duration::from_secs(10);
        clock.set_speed_at(Speed::Double, start + Duration::from_secs(2));
        assert_eq!(clock.host_deadline_at(due, start + Duration::from_secs(2)),
                   start + Duration::from_secs(6));
        clock.set_speed_at(Speed::Half, start + Duration::from_secs(4));
        assert_eq!(clock.host_deadline_at(due, start + Duration::from_secs(4)),
                   start + Duration::from_secs(12));
        assert_eq!(clock.host_deadline_at(due, start + Duration::from_secs(13)),
                   start + Duration::from_secs(13));
    }

    #[test]
    fn freeze_does_not_deliver_paused_time() {
        let mut clock = GuestClock::new();
        let before = clock.now();
        set_frozen(true);
        std::thread::sleep(Duration::from_millis(40));
        let during = clock.now().saturating_duration_since(before);
        assert!(during < Duration::from_millis(20), "guest time moved while frozen: {during:?}");
        set_frozen(false);
        std::thread::sleep(Duration::from_millis(40));
        let after = clock.now().saturating_duration_since(before);
        assert!(after >= Duration::from_millis(20), "guest time did not resume: {after:?}");
        assert!(after < Duration::from_millis(200), "paused time leaked into the guest: {after:?}");
    }

    #[test]
    fn speed_buttons_are_bounded_and_new_clock_is_normal() {
        assert_eq!(Speed::Quarter.step(false), Speed::Quarter);
        assert_eq!(Speed::Quadruple.step(true), Speed::Quadruple);
        assert_eq!(Speed::Normal.step(false), Speed::Half);
        assert_eq!(Speed::Normal.step(true), Speed::Double);
        assert_eq!(GuestClock::new().speed(), Speed::Normal);
    }
}
