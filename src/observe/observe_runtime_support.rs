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
    db.query_one("SELECT pg_advisory_lock($1)", &[&key])
        .await
        .context(acquire_error)?;
    let result = f().await;
    let unlock_result = db
        .query_one("SELECT pg_advisory_unlock($1)", &[&key])
        .await
        .context(release_error);
    match (result, unlock_result) {
        (Ok(value), Ok(_)) => Ok(value),
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Err(unlock_error)) => Err(unlock_error),
        (Err(error), Err(unlock_error)) => Err(anyhow!(
            "{error:#}\nsecondary unlock failure: {unlock_error:#}"
        )),
    }
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
