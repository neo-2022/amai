use super::*;

pub(super) async fn with_postgres_advisory_lock<T, F, Fut>(
    db: &Client,
    key: i64,
    acquire_error: &'static str,
    release_error: &'static str,
    f: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    crate::postgres::with_postgres_advisory_lock(db, key, acquire_error, release_error, f).await
}

pub(super) async fn timed_future<T, F>(
    timings: &mut serde_json::Map<String, Value>,
    label: &str,
    future: F,
) -> T
where
    F: Future<Output = T>,
{
    let started = Instant::now();
    let value = future.await;
    timings.insert(
        label.to_string(),
        Value::from(started.elapsed().as_millis() as u64),
    );
    value
}
