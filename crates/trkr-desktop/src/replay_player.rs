//! Bounded, GUI-driven playback. No device commands or output files.
use std::time::Duration;
#[derive(Default)]
pub struct Player {
    pub cursor: usize,
    pub cycle: usize,
    next: Duration,
    end_frame: bool,
    fast_frame: Option<Duration>,
}
pub struct Batch {
    pub clear: bool,
    pub rows: std::ops::Range<usize>,
    pub phase: &'static str,
}
impl Player {
    pub fn slow_motion(&self, elapsed: Duration, interval: f64, changed: bool) -> bool {
        changed
            && interval > 0.
            && elapsed >= self.next
            && elapsed.saturating_sub(self.next).as_secs_f64() < interval * 0.075
    }

    pub fn tick(&mut self, elapsed: Duration, interval: f64, count: usize) -> Batch {
        let wait = Duration::from_secs_f64(interval.clamp(0., 86400.));
        if wait.is_zero()
            && self.fast_frame.is_some_and(|last| {
                elapsed.saturating_sub(last) < Duration::from_secs_f64(1. / 60.)
            })
        {
            return Batch {
                clear: false,
                rows: self.cursor..self.cursor,
                phase: "Reading",
            };
        }
        if self.end_frame && !wait.is_zero() && elapsed < self.next {
            return Batch {
                clear: false,
                rows: self.cursor..self.cursor,
                phase: "Reading",
            };
        }
        let mut clear = false;
        if self.end_frame {
            self.cursor = 0;
            self.cycle += 1;
            self.next = elapsed;
            self.end_frame = false;
            clear = true;
        }
        let start = self.cursor;
        let phase = if wait.is_zero() {
            self.fast_frame = Some(elapsed);
            self.cursor = (self.cursor + 16).min(count);
            "Reading"
        } else {
            // Emit at 80% of each point, retaining the Python demo's move,
            // wait, read and post-read phases. Bound catch-up work per frame.
            while self.cursor < count
                && self.cursor - start < 16
                && elapsed >= self.next + wait.mul_f64(0.8)
            {
                self.cursor += 1;
                self.next += wait;
            }
            let fraction = elapsed.saturating_sub(self.next).as_secs_f64() / wait.as_secs_f64();
            if elapsed < self.next {
                "Reading"
            } else if fraction < 0.15 {
                "Moving"
            } else if fraction < 0.75 {
                "Waiting"
            } else {
                "Reading"
            }
        };
        self.end_frame = self.cursor == count;
        Batch {
            clear,
            rows: start..self.cursor,
            phase,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fast_replay_is_bounded_and_displays_last_frame_before_looping() {
        let mut p = Player::default();
        assert_eq!(p.tick(Duration::ZERO, 0., 25).rows, 0..16);
        assert_eq!(p.tick(Duration::from_millis(17), 0., 25).rows, 16..25);
        let next = p.tick(Duration::from_millis(34), 0., 25);
        assert!(next.clear);
        assert_eq!(next.rows, 0..16);
        assert_eq!(p.cycle, 1);
    }
    #[test]
    fn slow_axis_phase_and_post_read_wait_follow_python_timing() {
        let mut p = Player::default();
        p.tick(Duration::from_millis(50), 1., 2);
        assert!(p.slow_motion(Duration::from_millis(50), 1., true));
        assert!(!p.slow_motion(Duration::from_millis(80), 1., true));
        assert!(!p.slow_motion(Duration::from_millis(50), 1., false));
        assert_eq!(p.tick(Duration::from_millis(810), 1., 2).phase, "Reading");
        assert_eq!(p.tick(Duration::from_millis(1050), 1., 2).phase, "Moving");
    }
    #[test]
    fn timed_replay_emits_at_read_phase_and_bounds_catch_up() {
        let mut p = Player::default();
        assert!(p.tick(Duration::from_millis(700), 1., 100).rows.is_empty());
        assert_eq!(p.tick(Duration::from_millis(800), 1., 100).rows, 0..1);
        assert_eq!(p.tick(Duration::from_secs(90), 1., 100).rows, 1..17);
    }
}
