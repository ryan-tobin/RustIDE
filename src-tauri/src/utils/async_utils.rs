use crate::utils::{UtilError, UtilResult};
use std::future::Future;
use std::ops::Not;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Notify};
use tokio::time::{sleep, timeout, Instant};
use tracing::{debug, warn};

/// A cancellation token for async operations
#[derive(Debug, Clone)]
pub struct CancellationToken {
    inner: Arc<CancellationTokenInner>,
}

#[derive(Debug)]
struct CancellationTokenInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancellationToken {
    /// Create a new cancellation token
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancellationTokenInner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    /// Cancel last token
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
        debug!("Cancellation token cancelled");
    }

    /// Check if the token is cancelled
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Wait for cancellation
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        self.inner.notify.notified().await;
    }

    /// Run a future with cancellation support
    pub async fn run_with_cancellation<F, T>(&self, future: F) -> Result<T, UtilError>
    where
        F: Future<Output = T>,
    {
        tokio::select! {
            result = future => Ok(result),
            _ = self.cancelled() => Err(UtilError::Cancelled),
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a future with a timeout
pub async fn timeout_future<F, T>(future: F, duration: Duration) -> Result<T, UtilError>
where
    F: Future<Output = T>,
{
    timeout(duration, future)
        .await
        .map_err(|_| UtilError::Timeout { duration })
}

/// Retru an async operation with exponential backoff
pub async fn retry_async<F, Fut, T, E>(
    mut operation: F,
    max_attempts: usize,
    initial_delay: Duration,
    max_delay: Duration,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    let mut delay = initial_delay;

    for attempt in 1..=max_attempts {
        match operation().await {
            Ok(result) => {
                if attempt > 1 {
                    debug!("Operation succeeded on attempt {}", attempt);
                }
                return Ok(result);
            }
            Err(error) => {
                if attempt == max_attempts {
                    warn!(
                        "Operation failed after {} attempts: {:?}",
                        max_attempts, error
                    );
                    return Err(error);
                }

                debug!(
                    "Operation failed on attempt {}, retrying in {:?}",
                    attempt, delay
                );
                sleep(delay).await;

                delay = std::cmp::min(delay * 2, max_delay);
            }
        }
    }

    unreachable!()
}

/// Bacth async operations to avoid overwhelming the system
pub async fn batch_async<T, F, Fut, R>(
    items: Vec<T>,
    batch_size: usize,
    delay_between_batches: Duration,
    operation: F,
) -> Vec<R>
where
    F: Fn(T) -> Fut + Clone,
    Fut: Future<Output = R>,
{
    let mut results = Vec::with_capacity(items.len());

    for batch in items.chunks(batch_size) {
        let batch_futures: Vec<_> = batch
            .iter()
            .cloned()
            .map(|item| operation.clone()(item))
            .collect();

        let batch_results = futures::future::join_all(batch_futures).await;
        results.extend(batch_results);

        if delay_between_batches > Duration::ZERO {
            sleep(delay_between_batches).await;
        }
    }

    results
}

/// Rate limiter for async operations
pub struct RateLimiter {
    permits: Arc<tokio::sync::Semaphore>,
    refill_task: Option<tokio::task::JoinHandle<()>>,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(max_concurrent: usize, refill_rate: Duration) -> Self {
        let permits = Arc::new(tokio::sync::Semaphore::new(max_concurrent));

        let refill_permits = permits.clone();
        let refill_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(refill_rate);

            loop {
                interval.tick().await;

                if refill_permits.available_permits() < max_concurrent {
                    refill_permits.add_permits(1);
                }
            }
        });

        Self {
            permits,
            refill_task: Some(refill_task),
        }
    }

    /// Acquire a permit to perform an operation
    pub async fn acquire(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.permits.acquire().await.unwrap()
    }

    /// Execute an operation with rate limiting
    pub async fn execute<F, Fut, T>(&self, operation: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let _permit = self.acquire().await;
        operation().await
    }
}

impl Drop for RateLimiter {
    fn drop(&mut self) {
        if let Some(task) = self.refill_task.take() {
            task.abort();
        }
    }
}

/// Async task pool for managing background tasks
pub struct TaskPool {
    tasks: Arc<tokio::sync::RwLock<Vec<tokio::task::JoinHandle<()>>>>,
    shutdown_signal: Arc<Notify>,
}

impl TaskPool {
    /// Create a new task pool
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            shutdown_signal: Arc::new(Notify::new()),
        }
    }

    /// Spawn a task in the pool
    pub async fn spawn<F, Fut>(&self, task: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let shutdown_signal = self.shutdown_signal.clone();
        let tasks = self.tasks.clone();

        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = task() => {},
                _ = shutdown_signal.notified() => {
                    debug!("Task cancelled due to shutdown signal");
                }
            }
        });

        // Store the handle
        let mut task_list = tasks.write().await;
        task_list.push(handle);

        // Clean up completed tasks
        task_list.retain(|task| !task.is_finished());
    }

    /// Spawn a repeating task
    pub async fn spawn_interval<F, Fut>(&self, interval: Duration, task: F)
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let shutdown_signal = self.shutdown_signal.clone();

        self.spawn(move || async move {
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        task().await;
                    }
                    _ = shutdown_signal.notified() => {
                        break;
                    }
                }
            }
        })
        .await;
    }

    /// Get the number of active tasks
    pub async fn active_task_count(&self) -> usize {
        let tasks = self.tasks.read().await;
        tasks.iter().filter(|task| !task.is_finished()).count()
    }

    /// Shutdown all tasks gracefully
    pub async fn shutdown(&self) {
        debug!("Shutting down task pool");

        // Signal all tasks to stop
        self.shutdown_signal.notify_waiters();

        // Wait for all tasks to complete (with timeout)
        let tasks = {
            let mut task_list = self.tasks.write().await;
            std::mem::take(&mut *task_list)
        };

        for task in tasks {
            if let Err(e) = timeout(Duration::from_secs(5), task).await {
                warn!("Task did not complete within timeout: {:?}", e);
            }
        }

        debug!("Task pool shutdown complete");
    }
}

impl Default for TaskPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Async stream utils
pub mod stream {
    use super::*;
    use futures::Stream;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    /// Create a stream that yields items with a delay between them
    pub fn throttled_stream<S, T>(stream: S, delay: Duration) -> impl Stream<Item = T>
    where
        S: Stream<Item = T> + Unpin,
    {
        ThrottledStream::new(stream, delay)
    }

    struct ThrottledStream<S> {
        stream: S,
        delay: Duration,
        last_yield: Option<Instant>,
    }

    impl<S> ThrottledStream<S> {
        fn new(stream: S, delay: Duration) -> Self {
            Self {
                stream,
                delay,
                last_yield: None,
            }
        }
    }

    impl<S, T> Stream for ThrottledStream<S>
    where
        S: Stream<Item = T> + Unpin,
    {
        type Item = T;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let now = Instant::now();

            if let Some(last) = self.last_yield {
                if now.duration_since(last) < self.delay {
                    let waker = cx.waker().clone();
                    let remaining = self.delay - now.duration_since(last);
                    tokio::spawn(async move {
                        sleep(remaining).await;
                        waker.wake();
                    });
                    return Poll::Pending;
                }
            }

            match Pin::new(&mut self.stream).poll_next(cx) {
                Poll::Ready(Some(item)) => {
                    self.last_yield = Some(now);
                    Poll::Ready(Some(item))
                }
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cancellation_token() {
        let token = CancellationToken::new();
        let token_clone = token.clone();

        assert!(!token.is_cancelled());

        // Cancel in background
        tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            token_clone.cancel();
        });

        // Wait for cancellation
        token.cancelled().await;
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn test_timeout_future() {
        // Should succeed
        let result = timeout_future(async { 42 }, Duration::from_millis(100)).await;
        assert_eq!(result.unwrap(), 42);

        // Should timeout
        let result = timeout_future(
            async {
                sleep(Duration::from_millis(200)).await;
                42
            },
            Duration::from_millis(100),
        )
        .await;
        assert!(matches!(result, Err(UtilError::Timeout { .. })));
    }

    #[tokio::test]
    async fn test_retry_async() {
        let mut attempts = 0;

        let result = retry_async(
            || {
                attempts += 1;
                async move {
                    if attempts < 3 {
                        Err("not yet")
                    } else {
                        Ok("success")
                    }
                }
            },
            5,
            Duration::from_millis(10),
            Duration::from_millis(100),
        )
        .await;

        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts, 3);
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(2, Duration::from_millis(100));
        let start = Instant::now();

        // Execute 4 operations
        let results = futures::future::join_all((0..4).map(|i| {
            limiter.execute(move || async move {
                sleep(Duration::from_millis(50)).await;
                i
            })
        }))
        .await;

        assert_eq!(results, vec![0, 1, 2, 3]);

        // Should take at least 100ms due to rate limiting
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_task_pool() {
        let pool = TaskPool::new();
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // Spawn some tasks
        for _ in 0..5 {
            let counter_clone = counter.clone();
            pool.spawn(move || async move {
                sleep(Duration::from_millis(50)).await;
                counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
            .await;
        }

        // Wait for tasks to complete
        sleep(Duration::from_millis(100)).await;

        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 5);

        // Shutdown
        pool.shutdown().await;
    }
}
