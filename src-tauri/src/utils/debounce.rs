use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{sleep, Instant};
use tracing::debug;

/// Configuration for debouncing
#[derive(Debug, Clone)]
pub struct DebounceConfig {
    /// Delay before executing the debounced function
    pub delay: Duration,
    /// Maximum delay before forcing execution
    pub max_delay: Option<Duration>,
    /// Whether to execute on leading edge
    pub leading: bool,
    /// Whether to execute on trailing edge
    pub trailing: bool,
}

impl Default for DebounceConfig {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(300),
            max_delay: None,
            leading: false,
            trailing: true,
        }
    }
}

/// Debounced function state
struct DebounceState {
    last_call: Instant,
    first_call: Option<Instant>,
    pending: bool,
}

/// Debouncer for function calls
pub struct Debouncer {
    config: DebounceConfig,
    state: Arc<RwLock<HashMap<String, DebounceState>>>,
}

impl Debouncer {
    /// Create a new debounced with default configuration
    pub fn new(delay: Duration) -> Self {
        Self::with_config(DebounceConfig {
            delay,
            ..DebounceConfig::default()
        })
    }

    /// Create a new debouncer with custom configuration
    pub fn with_config(config: DebounceConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Debounce a function call
    pub async fn debounce<F, Fut>(&self, key: String, func: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let now = Instant::now();
        let should_execute_immediately;
        let should_schedule_execution;

        // Update state
        {
            let mut state = self.state.write().await;
            let entry = state.entry(key.clone()).or_insert(DebounceState {
                last_call: now,
                first_call: Some(now),
                pending: false,
            });

            let time_since_last = now.duration_since(entry.last_call);
            let time_since_first = entry
                .first_call
                .map(|first| now.duration_since(first))
                .unwrap_or(Duration::ZERO);

            // Check if we should execute immediately (leading edge)
            should_execute_immediately = self.config.leading && !entry.pending;

            // Check if we should schedule execution
            should_schedule_execution = self.config.trailing
                && (time_since_last >= self.config.delay
                    || self
                        .config
                        .max_delay
                        .map_or(false, |max| time_since_first >= max));

            entry.last_call = now;
            entry.pending = true;

            // Reset first_call if enough time has passed
            if time_since_last >= self.config.delay {
                entry.first_call = Some(now);
            }
        }

        if should_execute_immediately {
            debug!("Executing function immediately (leading edge): {}", key);
            func().await;

            // Mark as not pending
            let mut state = self.state.write().await;
            if let Some(entry) = state.get_mut(&key) {
                entry.pending = false;
            }
        } else if should_schedule_execution {
            debug!("Scheduling function execution: {}", key);
            self.schedule_execution(key, func).await;
        } else {
            // Just schedule the function
            self.schedule_execution(key, func).await;
        }
    }

    /// Schedule function execution after delay
    async fn schedule_execution<F, Fut>(&self, key: String, func: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let state = self.state.clone();
        let delay = self.config.delay;
        let max_delay = self.config.max_delay;

        tokio::spawn(async move {
            sleep(delay).await;

            let should_execute = {
                let mut state_guard = state.write().await;
                if let Some(entry) = state_guard.get_mut(&key) {
                    let now = Instant::now();
                    let time_since_last = now.duration_since(entry.last_call);
                    let time_since_first = entry
                        .first_call
                        .map(|first| now.duration_since(first))
                        .unwrap_or(Duration::ZERO);

                    let should_exec = time_since_last >= delay
                        || max_delay.map_or(false, |max| time_since_first >= max);

                    if should_exec {
                        entry.pending = false;
                        state_guard.remove(&key);
                    }

                    should_exec
                } else {
                    false
                }
            };

            if should_execute {
                debug!("Executing debounced function: {}", key);
                func().await;
            }
        });
    }

    /// Cancel pending execution for a key
    pub async fn cancel(&self, key: &str) {
        let mut state = self.state.write().await;
        state.remove(key);
        debug!("Cancelled debounced function: {}", key);
    }

    /// Check if a key has pending execution
    pub async fn is_pending(&self, key: &str) -> bool {
        let state = self.state.read().await;
        state.get(key).map_or(false, |entry| entry.pending)
    }

    /// Clear all pending executions
    pub async fn clear_all(&self) {
        let mut state = self.state.write().await;
        state.clear();
        debug!("Cleared all debounced functions");
    }
}

/// Simple debounce function for one-off use
pub async fn debounce<F, Fut>(key: String, delay: Duration, func: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    static GLOBAL_DEBOUNCER: once_cell::sync::Lazy<Debouncer> =
        once_cell::sync::Lazy::new(|| Debouncer::new(Duration::from_millis(300)));

    GLOBAL_DEBOUNCER.debounce(key, func).await;
}

/// Debounce configuration builder
pub struct DebounceBuilder {
    config: DebounceConfig,
}

impl DebounceBuilder {
    /// Create a new debounce builder
    pub fn new() -> Self {
        Self {
            config: DebounceConfig::default(),
        }
    }

    /// Set the delay
    pub fn delay(mut self, delay: Duration) -> Self {
        self.config.delay = delay;
        self
    }

    /// Set the maximum delay
    pub fn max_delay(mut self, max_delay: Duration) -> Self {
        self.config.max_delay = Some(max_delay);
        self
    }

    /// Enable leading edge execution
    pub fn leading(mut self) -> Self {
        self.config.leading = true;
        self
    }

    /// Disable trailing edge execution
    pub fn no_trailing(mut self) -> Self {
        self.config.trailing = false;
        self
    }

    /// Build the debouncer
    pub fn build(self) -> Debouncer {
        Debouncer::with_config(self.config)
    }
}

impl Default for DebounceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_basic_debounce() {
        let debouncer = Debouncer::new(Duration::from_millis(100));
        let counter = Arc::new(AtomicUsize::new(0));

        // Call multiple times quickly
        for _ in 0..5 {
            let counter_clone = counter.clone();
            debouncer
                .debounce("test".to_string(), move || {
                    let counter = counter_clone;
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .await;
            sleep(Duration::from_millis(10)).await;
        }

        // Wait for debounce to complete
        sleep(Duration::from_millis(200)).await;

        // Should only execute once
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_leading_edge() {
        let config = DebounceConfig {
            delay: Duration::from_millis(100),
            leading: true,
            trailing: false,
            ..DebounceConfig::default()
        };

        let debouncer = Debouncer::with_config(config);
        let counter = Arc::new(AtomicUsize::new(0));

        // First call should execute immediately
        let counter_clone = counter.clone();
        debouncer
            .debounce("test".to_string(), move || {
                let counter = counter_clone;
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })
            .await;

        // Should execute immediately
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Subsequent calls within delay period should not execute
        for _ in 0..3 {
            let counter_clone = counter.clone();
            debouncer
                .debounce("test".to_string(), move || {
                    let counter = counter_clone;
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .await;
            sleep(Duration::from_millis(50)).await;
        }

        // Should still be 1
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
