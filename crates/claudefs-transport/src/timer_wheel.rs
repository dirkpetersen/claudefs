//! Hierarchical timer wheel for managing concurrent request timeouts.

use std::time::Instant;

/// Opaque handle for a registered timer, used to cancel it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimerHandle(u64);

/// User-defined data attached to each timer.
#[derive(Debug, Clone)]
pub struct TimerToken {
    /// Timer identifier.
    pub id: u64,
    /// Arbitrary user-defined tag (e.g. request id).
    pub tag: u64,
}

/// Returned by tick() for each fired timer.
#[derive(Debug, Clone)]
pub struct TimerFired {
    /// The handle for this timer.
    pub handle: TimerHandle,
    /// The token that was registered with this timer.
    pub token: TimerToken,
    /// When the timer was scheduled.
    pub scheduled_at: Instant,
    /// When the timer actually fired.
    pub fired_at: Instant,
}

/// Timer wheel configuration.
#[derive(Debug, Clone)]
pub struct TimerWheelConfig {
    /// Fine slot duration in milliseconds. Default: 10.
    pub fine_slot_ms: u64,
    /// Number of fine slots. Default: 100.
    pub fine_slots: usize,
    /// Coarse slot duration in milliseconds. Default: 1000.
    pub coarse_slot_ms: u64,
    /// Number of coarse slots. Default: 64.
    pub coarse_slots: usize,
}

impl Default for TimerWheelConfig {
    fn default() -> Self {
        Self {
            fine_slot_ms: 10,
            fine_slots: 100,
            coarse_slot_ms: 1000,
            coarse_slots: 64,
        }
    }
}

/// Timer wheel statistics.
#[derive(Debug, Clone, Default)]
pub struct TimerWheelStats {
    /// Number of currently active (not yet fired, not cancelled) timers.
    pub active_timers: usize,
    /// Total timers ever inserted.
    pub timers_inserted: u64,
    /// Total timers fired.
    pub timers_fired: u64,
    /// Total timers cancelled.
    pub timers_cancelled: u64,
    /// Total tick() calls processed.
    pub ticks_processed: u64,
}

struct TimerEntry {
    handle: TimerHandle,
    token: TimerToken,
    deadline: Instant,
    scheduled_at: Instant,
    cancelled: bool,
}

/// Hierarchical timer wheel. NOT thread-safe — wrap in Mutex if needed.
pub struct TimerWheel {
    config: TimerWheelConfig,
    timers: Vec<TimerEntry>,
    next_id: u64,
    stats: TimerWheelStats,
    last_tick: Instant,
}

impl TimerWheel {
    /// Creates a new timer wheel with the given configuration and start time.
    pub fn new(config: TimerWheelConfig, start: Instant) -> Self {
        Self {
            config,
            timers: Vec::new(),
            next_id: 0,
            stats: TimerWheelStats::default(),
            last_tick: start,
        }
    }

    /// Inserts a timer that will fire at the given deadline.
    ///
    /// Returns a handle that can be used to cancel the timer.
    pub fn insert(&mut self, deadline: Instant, token: TimerToken) -> TimerHandle {
        let handle = TimerHandle(self.next_id);
        self.next_id += 1;

        let entry = TimerEntry {
            handle,
            token,
            deadline,
            scheduled_at: Instant::now(),
            cancelled: false,
        };

        self.timers.push(entry);
        self.stats.active_timers += 1;
        self.stats.timers_inserted += 1;

        handle
    }

    /// Cancels a timer by its handle.
    ///
    /// Returns true if the timer was found and cancelled (or was already cancelled).
    /// Returns false if the timer handle does not exist or was already fired.
    pub fn cancel(&mut self, handle: TimerHandle) -> bool {
        if let Some(entry) = self.timers.iter_mut().find(|e| e.handle == handle) {
            if entry.cancelled {
                return false;
            }
            entry.cancelled = true;
            self.stats.active_timers = self.stats.active_timers.saturating_sub(1);
            self.stats.timers_cancelled += 1;
            true
        } else {
            false
        }
    }

    /// Advances the timer wheel to the current time and returns all fired timers.
    ///
    /// Timers whose deadline has passed will be returned in the vector.
    /// Cancelled timers are silently dropped.
    pub fn tick(&mut self, now: Instant) -> Vec<TimerFired> {
        self.stats.ticks_processed += 1;

        let mut fired = Vec::new();
        let mut remaining = Vec::new();

        for entry in self.timers.drain(..) {
            if entry.cancelled {
                continue;
            }

            if entry.deadline <= now {
                fired.push(TimerFired {
                    handle: entry.handle,
                    token: entry.token,
                    scheduled_at: entry.scheduled_at,
                    fired_at: now,
                });
                self.stats.timers_fired += 1;
                self.stats.active_timers = self.stats.active_timers.saturating_sub(1);
            } else {
                remaining.push(entry);
            }
        }

        self.timers = remaining;
        self.last_tick = now;

        fired
    }

    /// Returns the number of active (not fired, not cancelled) timers.
    pub fn active_count(&self) -> usize {
        self.stats.active_timers
    }

    /// Returns statistics about the timer wheel.
    pub fn stats(&self) -> TimerWheelStats {
        self.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn token(id: u64) -> TimerToken {
        TimerToken { id, tag: id * 10 }
    }

    fn make_wheel() -> TimerWheel {
        TimerWheel::new(TimerWheelConfig::default(), Instant::now())
    }

    fn advance(base: Instant, millis: u64) -> Instant {
        base + ms(millis)
    }

    #[test]
    fn test_insert_and_tick_fires_timer() {
        let mut wheel = make_wheel();
        let start = Instant::now();
        let deadline = advance(start, 100);

        let handle = wheel.insert(deadline, token(1));
        assert_eq!(wheel.active_count(), 1);

        let fired = wheel.tick(advance(start, 150));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].handle, handle);
        assert_eq!(fired[0].token.id, 1);
        assert_eq!(wheel.active_count(), 0);
    }

    #[test]
    fn test_timer_not_fired_before_deadline() {
        let mut wheel = make_wheel();
        let start = Instant::now();
        let deadline = advance(start, 100);

        wheel.insert(deadline, token(1));

        let fired = wheel.tick(advance(start, 50));
        assert!(fired.is_empty());
        assert_eq!(wheel.active_count(), 1);
    }

    #[test]
    fn test_cancel_timer() {
        let mut wheel = make_wheel();
        let start = Instant::now();
        let deadline = advance(start, 100);

        let handle = wheel.insert(deadline, token(1));
        assert_eq!(wheel.active_count(), 1);

        assert!(wheel.cancel(handle));
        assert_eq!(wheel.active_count(), 0);

        let fired = wheel.tick(advance(start, 150));
        assert!(fired.is_empty());
    }

    #[test]
    fn test_cancel_nonexistent_returns_false() {
        let mut wheel = make_wheel();
        let fake_handle = TimerHandle(999);

        assert!(!wheel.cancel(fake_handle));
    }

    #[test]
    fn test_multiple_timers_same_deadline() {
        let mut wheel = make_wheel();
        let start = Instant::now();
        let deadline = advance(start, 100);

        let h1 = wheel.insert(deadline, token(1));
        let h2 = wheel.insert(deadline, token(2));
        let h3 = wheel.insert(deadline, token(3));

        assert_eq!(wheel.active_count(), 3);

        let fired = wheel.tick(advance(start, 150));
        assert_eq!(fired.len(), 3);

        let handles: std::collections::HashSet<_> = fired.iter().map(|f| f.handle).collect();
        assert!(handles.contains(&h1));
        assert!(handles.contains(&h2));
        assert!(handles.contains(&h3));
    }

    #[test]
    fn test_timers_fire_in_correct_order() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(1));
        wheel.insert(advance(start, 200), token(2));

        let fired1 = wheel.tick(advance(start, 150));
        assert_eq!(fired1.len(), 1);
        assert_eq!(fired1[0].token.id, 1);
        assert_eq!(wheel.active_count(), 1);

        let fired2 = wheel.tick(advance(start, 250));
        assert_eq!(fired2.len(), 1);
        assert_eq!(fired2[0].token.id, 2);
        assert_eq!(wheel.active_count(), 0);
    }

    #[test]
    fn test_many_timers() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        for i in 0..1000 {
            wheel.insert(advance(start, 100 + i), token(i));
        }

        assert_eq!(wheel.active_count(), 1000);

        let fired = wheel.tick(advance(start, 2000));
        assert_eq!(fired.len(), 1000);
        assert_eq!(wheel.active_count(), 0);
    }

    #[test]
    fn test_stats_inserted_fired() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(1));
        wheel.insert(advance(start, 200), token(2));

        let stats = wheel.stats();
        assert_eq!(stats.timers_inserted, 2);
        assert_eq!(stats.timers_fired, 0);

        wheel.tick(advance(start, 300));

        let stats = wheel.stats();
        assert_eq!(stats.timers_fired, 2);
    }

    #[test]
    fn test_stats_cancelled() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let h = wheel.insert(advance(start, 100), token(1));
        wheel.cancel(h);

        let stats = wheel.stats();
        assert_eq!(stats.timers_cancelled, 1);
    }

    #[test]
    fn test_active_count_decrements_on_fire() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(1));
        assert_eq!(wheel.active_count(), 1);

        wheel.tick(advance(start, 150));
        assert_eq!(wheel.active_count(), 0);
    }

    #[test]
    fn test_active_count_decrements_on_cancel() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let h = wheel.insert(advance(start, 100), token(1));
        assert_eq!(wheel.active_count(), 1);

        wheel.cancel(h);
        assert_eq!(wheel.active_count(), 0);
    }

    #[test]
    fn test_tick_same_instant_idempotent() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(1));

        let fired1 = wheel.tick(advance(start, 150));
        assert_eq!(fired1.len(), 1);

        let fired2 = wheel.tick(advance(start, 150));
        assert!(fired2.is_empty());
    }

    #[test]
    fn test_large_deadline_fires_correctly() {
        let mut wheel = make_wheel();
        let start = Instant::now();
        let deadline = advance(start, 60_000);

        wheel.insert(deadline, token(1));

        let fired1 = wheel.tick(advance(start, 30_000));
        assert!(fired1.is_empty());

        let fired2 = wheel.tick(advance(start, 70_000));
        assert_eq!(fired2.len(), 1);
    }

    #[test]
    fn test_timer_token_preserved() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(42));

        let fired = wheel.tick(advance(start, 150));
        assert_eq!(fired[0].token.id, 42);
        assert_eq!(fired[0].token.tag, 420);
    }

    #[test]
    fn test_fired_at_timestamp_set() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(1));

        let tick_time = advance(start, 150);
        let fired = wheel.tick(tick_time);

        assert_eq!(fired[0].fired_at, tick_time);
    }

    #[test]
    fn test_scheduled_at_preserved() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let before = Instant::now();
        wheel.insert(advance(start, 100), token(1));
        let after = Instant::now();

        let fired = wheel.tick(advance(start, 150));
        assert!(fired[0].scheduled_at >= before);
        assert!(fired[0].scheduled_at <= after);
    }

    #[test]
    fn test_default_config_values() {
        let config = TimerWheelConfig::default();

        assert_eq!(config.fine_slot_ms, 10);
        assert_eq!(config.fine_slots, 100);
        assert_eq!(config.coarse_slot_ms, 1000);
        assert_eq!(config.coarse_slots, 64);
    }

    #[test]
    fn test_zero_active_count_initially() {
        let wheel = make_wheel();
        assert_eq!(wheel.active_count(), 0);
    }

    #[test]
    fn test_handle_uniqueness() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let mut handles = std::collections::HashSet::new();
        for i in 0..100 {
            let h = wheel.insert(advance(start, 100 + i), token(i));
            assert!(handles.insert(h));
        }
    }

    #[test]
    fn test_cancel_already_fired_returns_false() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let h = wheel.insert(advance(start, 100), token(1));
        wheel.tick(advance(start, 150));

        assert!(!wheel.cancel(h));
    }

    #[test]
    fn test_multiple_ticks_accumulate() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(1));
        wheel.tick(advance(start, 150));

        wheel.insert(advance(start, 200), token(2));
        wheel.tick(advance(start, 250));

        let stats = wheel.stats();
        assert_eq!(stats.timers_fired, 2);
        assert_eq!(stats.ticks_processed, 2);
    }

    #[test]
    fn test_timer_at_exact_deadline_fires() {
        let mut wheel = make_wheel();
        let start = Instant::now();
        let deadline = advance(start, 100);

        wheel.insert(deadline, token(1));

        let fired = wheel.tick(deadline);
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn test_cancelled_timer_not_in_fired_list() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let h1 = wheel.insert(advance(start, 100), token(1));
        wheel.insert(advance(start, 100), token(2));

        wheel.cancel(h1);

        let fired = wheel.tick(advance(start, 150));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].token.id, 2);
    }

    #[test]
    fn test_stats_ticks_processed() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.tick(advance(start, 100));
        wheel.tick(advance(start, 200));
        wheel.tick(advance(start, 300));

        let stats = wheel.stats();
        assert_eq!(stats.ticks_processed, 3);
    }

    #[test]
    fn test_fire_multiple_batches() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(1));
        wheel.insert(advance(start, 200), token(2));
        wheel.insert(advance(start, 300), token(3));

        let fired1 = wheel.tick(advance(start, 150));
        assert_eq!(fired1.len(), 1);

        let fired2 = wheel.tick(advance(start, 250));
        assert_eq!(fired2.len(), 1);

        let fired3 = wheel.tick(advance(start, 350));
        assert_eq!(fired3.len(), 1);
    }

    #[test]
    fn test_past_deadline_fires_on_next_tick() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let past_deadline = advance(start, 50);
        wheel.insert(past_deadline, token(1));

        let fired = wheel.tick(advance(start, 100));
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn test_all_timers_fire_eventually() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        for i in 0..10 {
            wheel.insert(advance(start, (i + 1) * 100), token(i));
        }

        let fired = wheel.tick(advance(start, 5000));
        assert_eq!(fired.len(), 10);
    }

    #[test]
    fn test_cancel_subset_of_timers() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let h1 = wheel.insert(advance(start, 100), token(1));
        let h2 = wheel.insert(advance(start, 100), token(2));
        let h3 = wheel.insert(advance(start, 100), token(3));

        wheel.cancel(h1);
        wheel.cancel(h3);

        let fired = wheel.tick(advance(start, 150));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].handle, h2);
    }

    #[test]
    fn test_token_id_and_tag_preserved() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let tok = TimerToken { id: 123, tag: 456 };
        wheel.insert(advance(start, 100), tok);

        let fired = wheel.tick(advance(start, 150));
        assert_eq!(fired[0].token.id, 123);
        assert_eq!(fired[0].token.tag, 456);
    }

    #[test]
    fn test_no_duplicate_fires() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(1));

        wheel.tick(advance(start, 150));
        wheel.tick(advance(start, 200));
        wheel.tick(advance(start, 250));

        let stats = wheel.stats();
        assert_eq!(stats.timers_fired, 1);
    }

    #[test]
    fn test_insert_after_tick() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        wheel.insert(advance(start, 100), token(1));
        wheel.tick(advance(start, 150));

        wheel.insert(advance(start, 200), token(2));

        let fired = wheel.tick(advance(start, 250));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].token.id, 2);
    }

    #[test]
    fn test_empty_tick_returns_empty_vec() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let fired = wheel.tick(advance(start, 100));
        assert!(fired.is_empty());
    }

    #[test]
    fn test_cancel_twice_returns_false_second_time() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let h = wheel.insert(advance(start, 100), token(1));

        assert!(wheel.cancel(h));
        assert!(!wheel.cancel(h));
    }

    #[test]
    fn test_stats_timers_inserted_counts_all() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        for i in 0..50 {
            wheel.insert(advance(start, 100 + i), token(i));
        }

        let stats = wheel.stats();
        assert_eq!(stats.timers_inserted, 50);
    }

    #[test]
    fn test_wheel_with_custom_config() {
        let config = TimerWheelConfig {
            fine_slot_ms: 5,
            fine_slots: 200,
            coarse_slot_ms: 500,
            coarse_slots: 128,
        };
        let start = Instant::now();
        let mut wheel = TimerWheel::new(config, start);

        wheel.insert(advance(start, 100), token(1));
        let fired = wheel.tick(advance(start, 150));

        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn test_cancel_after_partial_fire() {
        let mut wheel = make_wheel();
        let start = Instant::now();

        let h1 = wheel.insert(advance(start, 100), token(1));
        let h2 = wheel.insert(advance(start, 300), token(2));

        wheel.tick(advance(start, 150));

        assert!(wheel.cancel(h2));
        assert!(!wheel.cancel(h1));
    }

    #[test]
    fn test_multiple_wheels_independent() {
        let start = Instant::now();
        let mut wheel1 = make_wheel();
        let mut wheel2 = make_wheel();

        let h1 = wheel1.insert(advance(start, 100), token(1));
        let h2 = wheel2.insert(advance(start, 100), token(2));

        // Handles are only unique within the same wheel — don't compare across wheels.
        // Verify independence instead:
        let fired1 = wheel1.tick(advance(start, 150));
        let fired2 = wheel2.tick(advance(start, 150));

        assert_eq!(fired1.len(), 1);
        assert_eq!(fired2.len(), 1);
        assert_eq!(fired1[0].token.id, 1);
        assert_eq!(fired2[0].token.id, 2);
        // Verify handles are recorded correctly within each wheel's context
        assert_eq!(fired1[0].handle, h1);
        assert_eq!(fired2[0].handle, h2);
    }
}
