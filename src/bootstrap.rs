use crate::{compatibility, config::AppConfig, edge_cache, nats, postgres, qdrant, s3};
use anyhow::{Result, anyhow};
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

pub async fn bootstrap_stack(cfg: &AppConfig) -> Result<()> {
    let db = retry_async("postgres bootstrap", 30, Duration::from_secs(2), || async {
        let db = postgres::connect_admin(cfg).await?;
        postgres::bootstrap_schema(&db, cfg).await?;
        compatibility::bootstrap_meta(cfg, &db).await?;
        Ok(db)
    })
    .await?;

    let qdrant_client = retry_async("qdrant bootstrap", 30, Duration::from_secs(2), || async {
        let client = qdrant::connect(cfg)?;
        qdrant::bootstrap_collections(&client, cfg).await?;
        Ok(client)
    })
    .await?;

    let s3_client = retry_async("s3 bootstrap", 30, Duration::from_secs(2), || async {
        let client = s3::connect(cfg).await?;
        s3::ensure_buckets(&client, cfg).await?;
        Ok(client)
    })
    .await?;

    let nats_client = retry_async("nats bootstrap", 30, Duration::from_secs(2), || async {
        let client = nats::connect(cfg).await?;
        nats::ensure_streams(client.clone()).await?;
        Ok(client)
    })
    .await?;

    edge_cache::ensure(&cfg.edge_cache_path)?;
    compatibility::assert_supported(cfg).await?;
    tracing::info!(
        postgres = true,
        qdrant = true,
        s3 = true,
        nats = true,
        memory_embed_model = %cfg.memory_embed_model,
        edge_cache = %cfg.edge_cache_path.display(),
        "bootstrap stack completed"
    );
    drop(db);
    drop(qdrant_client);
    drop(s3_client);
    drop(nats_client);
    Ok(())
}

async fn retry_async<F, Fut, T>(
    label: &str,
    attempts: usize,
    delay: Duration,
    mut operation: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut last_error = None;
    for attempt in 1..=attempts {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) => {
                tracing::warn!(
                    attempt,
                    attempts,
                    %label,
                    error = %error,
                    "bootstrap step not ready yet"
                );
                last_error = Some(error);
                if attempt < attempts {
                    sleep(delay).await;
                }
            }
        }
    }

    Err(last_error
        .map(|error| error.context(format!("{label} failed after {attempts} attempts")))
        .unwrap_or_else(|| anyhow!("{label} failed without captured error")))
}
