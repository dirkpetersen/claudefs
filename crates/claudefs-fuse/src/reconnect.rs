use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Reconnecting { attempt: u32 },
    Failed,
}

pub struct ReconnectConfig {
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_attempts: u32,
    pub backoff_multiplier: f64,
    pub jitter_fraction: f64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_delay_ms: 100,
            max_delay_ms: 30_000,
            max_attempts: 10,
            backoff_multiplier: 2.0,
            jitter_fraction: 0.1,
        }
    }
}

#[derive(Debug)]
pub struct ReconnectError {
    pub message: String,
    pub attempt: u32,
}

impl fmt::Display for ReconnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Reconnect failed after {} attempts: {}",
            self.attempt, self.message
        )
    }
}

impl std::error::Error for ReconnectError {}

pub struct ReconnectState {
    pub config: ReconnectConfig,
    pub state: ConnectionState,
    pub attempt: u32,
    pub last_delay_ms: u64,
}

impl ReconnectState {
    pub fn new(config: ReconnectConfig) -> Self {
        tracing::debug!(
            "Initializing reconnect state: initial_delay={}ms, max_delay={}ms, max_attempts={}",
            config.initial_delay_ms,
            config.max_delay_ms,
            config.max_attempts
        );

        if config.initial_delay_ms > config.max_delay_ms {
            tracing::warn!(
                "initial_delay_ms ({}) > max_delay_ms ({})",
                config.initial_delay_ms,
                config.max_delay_ms
            );
        }

        Self {
            config,
            state: ConnectionState::Disconnected,
            attempt: 0,
            last_delay_ms: 0,
        }
    }

    pub fn on_connected(&mut self) {
        self.state = ConnectionState::Connected;
        self.attempt = 0;
        self.last_delay_ms = 0;
        tracing::info!("Connection established, reset reconnect state");
    }

    pub fn on_disconnected(&mut self) {
        let attempt = if self.attempt == 0 {
            1
        } else {
            self.attempt + 1
        };
        self.state = ConnectionState::Reconnecting { attempt };
        tracing::warn!(
            "Connection lost, entering reconnect state (attempt {})",
            attempt
        );
    }

    pub fn next_delay_ms(&mut self) -> u64 {
        let base_delay = if self.attempt == 0 {
            self.config.initial_delay_ms
        } else {
            let calculated = (self.config.initial_delay_ms as f64
                * self.config.backoff_multiplier.powi(self.attempt as i32))
                as u64;
            calculated.min(self.config.max_delay_ms)
        };

        let jitter = if self.config.jitter_fraction > 0.0 {
            let jitter_range = (base_delay as f64 * self.config.jitter_fraction) as u64;
            let jitter_val = rand_jitter(jitter_range);
            jitter_range.saturating_sub(jitter_val)
        } else {
            0
        };

        self.last_delay_ms = base_delay.saturating_sub(jitter).max(1);

        tracing::debug!(
            "Next reconnect delay: {}ms (base={}, jitter={})",
            self.last_delay_ms,
            base_delay,
            jitter
        );

        self.last_delay_ms
    }

    pub fn should_give_up(&self) -> bool {
        self.attempt >= self.config.max_attempts
    }

    pub fn advance_attempt(&mut self) {
        self.attempt += 1;

        if self.should_give_up() {
            self.state = ConnectionState::Failed;
            tracing::error!(
                "Max reconnect attempts ({}) exceeded, giving up",
                self.config.max_attempts
            );
        } else {
            self.state = ConnectionState::Reconnecting {
                attempt: self.attempt,
            };
            tracing::debug!("Advanced to reconnect attempt {}", self.attempt);
        }
    }

    pub fn is_retrying(&self) -> bool {
        matches!(self.state, ConnectionState::Reconnecting { .. })
    }
}

fn rand_jitter(max: u64) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    if max == 0 {
        return 0;
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);

    (nanos as u64) % max
}

pub fn retry_with_backoff<T, E, F>(state: &mut ReconnectState, mut op: F) -> Result<T, E>
where
    F: FnMut() -> std::result::Result<T, E>,
    E: fmt::Debug,
{
    loop {
        match op() {
            Ok(result) => {
                state.on_connected();
                return Ok(result);
            }
            Err(e) => {
                tracing::debug!("Operation failed: {:?}", e);

                if !state.is_retrying() && !state.should_give_up() {
                    state.on_disconnected();
                }

                if state.should_give_up() {
                    tracing::error!("Retry failed: max attempts exceeded");
                    return Err(e);
                }

                let delay = state.next_delay_ms();
                tracing::info!(
                    "Retrying after {}ms (attempt {}/{})",
                    delay,
                    state.attempt + 1,
                    state.config.max_attempts
                );

                std::thread::sleep(std::time::Duration::from_millis(delay));
                state.advance_attempt();
            }
        }
    }
}

impl Default for ReconnectState {
    fn default() -> Self {
        Self::new(ReconnectConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ReconnectConfig {
        ReconnectConfig {
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            max_attempts: 5,
            backoff_multiplier: 2.0,
            jitter_fraction: 0.0,
        }
    }

    #[test]
    fn default_config_has_valid_ranges() {
        let config = ReconnectConfig::default();
        assert!(
            config.initial_delay_ms < config.max_delay_ms,
            "initial should be < max"
        );
        assert!(
            config.backoff_multiplier > 1.0,
            "multiplier should be > 1.0"
        );
    }

    #[test]
    fn new_state_is_disconnected() {
        let state = ReconnectState::new(test_config());
        assert_eq!(state.state, ConnectionState::Disconnected);
    }

    #[test]
    fn on_connected_sets_state_to_connected() {
        let mut state = ReconnectState::new(test_config());
        state.attempt = 5;
        state.on_connected();

        assert_eq!(state.state, ConnectionState::Connected);
        assert_eq!(state.attempt, 0);
    }

    #[test]
    fn on_disconnected_transitions_to_reconnecting() {
        let mut state = ReconnectState::new(test_config());
        state.on_disconnected();

        assert!(matches!(
            state.state,
            ConnectionState::Reconnecting { attempt: 1 }
        ));
    }

    #[test]
    fn next_delay_ms_returns_initial_delay_on_first_attempt() {
        let mut state = ReconnectState::new(ReconnectConfig {
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            max_attempts: 5,
            backoff_multiplier: 2.0,
            jitter_fraction: 0.0,
        });

        state.attempt = 0;
        let delay = state.next_delay_ms();

        assert_eq!(delay, 100);
    }

    #[test]
    fn next_delay_ms_doubles_on_second_attempt() {
        let mut state = ReconnectState::new(ReconnectConfig {
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            max_attempts: 5,
            backoff_multiplier: 2.0,
            jitter_fraction: 0.0,
        });

        state.attempt = 1;
        let delay = state.next_delay_ms();

        assert_eq!(delay, 200);
    }

    #[test]
    fn next_delay_ms_is_capped_at_max_delay() {
        let mut state = ReconnectState::new(ReconnectConfig {
            initial_delay_ms: 100,
            max_delay_ms: 500,
            max_attempts: 10,
            backoff_multiplier: 2.0,
            jitter_fraction: 0.0,
        });

        state.attempt = 10;
        let delay = state.next_delay_ms();

        assert_eq!(delay, 500, "Should be capped at max_delay_ms");
    }

    #[test]
    fn next_delay_ms_with_zero_jitter_is_exactly_delay() {
        let mut state = ReconnectState::new(ReconnectConfig {
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            max_attempts: 5,
            backoff_multiplier: 2.0,
            jitter_fraction: 0.0,
        });

        state.attempt = 2;
        let delay = state.next_delay_ms();

        assert_eq!(delay, 400);
    }

    #[test]
    fn should_give_up_returns_false_at_zero_attempts() {
        let state = ReconnectState::new(ReconnectConfig {
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            max_attempts: 5,
            backoff_multiplier: 2.0,
            jitter_fraction: 0.0,
        });

        assert!(!state.should_give_up());
    }

    #[test]
    fn should_give_up_returns_true_when_attempt_exceeds_max() {
        let mut state = ReconnectState::new(ReconnectConfig {
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            max_attempts: 5,
            backoff_multiplier: 2.0,
            jitter_fraction: 0.0,
        });

        state.attempt = 5;

        assert!(state.should_give_up());
    }

    #[test]
    fn advance_attempt_increments_counter() {
        let mut state = ReconnectState::new(test_config());

        state.advance_attempt();

        assert_eq!(state.attempt, 1);
    }

    #[test]
    fn advance_attempt_transitions_to_failed_when_max_exceeded() {
        let mut state = ReconnectState::new(ReconnectConfig {
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            max_attempts: 5,
            backoff_multiplier: 2.0,
            jitter_fraction: 0.0,
        });

        state.attempt = 4;
        state.advance_attempt();

        assert_eq!(state.state, ConnectionState::Failed);
    }

    #[test]
    fn is_retrying_returns_true_in_reconnecting_state() {
        let mut state = ReconnectState::new(test_config());

        state.state = ConnectionState::Reconnecting { attempt: 1 };

        assert!(state.is_retrying());
    }

    #[test]
    fn is_retrying_returns_false_in_connected_state() {
        let mut state = ReconnectState::new(test_config());

        state.state = ConnectionState::Connected;

        assert!(!state.is_retrying());
    }

    #[test]
    fn is_retrying_returns_false_in_failed_state() {
        let mut state = ReconnectState::new(test_config());

        state.state = ConnectionState::Failed;

        assert!(!state.is_retrying());
    }

    #[test]
    fn retry_with_backoff_success_on_first_try() {
        let mut state = ReconnectState::new(test_config());

        let result = retry_with_backoff(&mut state, || Ok::<_, ()>(42));

        assert_eq!(result, Ok(42));
        assert_eq!(state.state, ConnectionState::Connected);
    }

    #[test]
    fn retry_with_backoff_retries_on_failure() {
        let mut state = ReconnectState::new(ReconnectConfig {
            initial_delay_ms: 10,
            max_delay_ms: 100,
            max_attempts: 3,
            backoff_multiplier: 1.0,
            jitter_fraction: 0.0,
        });

        let mut attempts = 0;
        let result = retry_with_backoff(&mut state, || {
            attempts += 1;
            if attempts < 2 {
                Err(())
            } else {
                Ok(42)
            }
        });

        assert_eq!(result, Ok(42));
        assert!(attempts >= 2);
    }
}
