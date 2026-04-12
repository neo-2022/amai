use crate::config::AppConfig;
use crate::postgres;
use anyhow::Result;
use async_nats::jetstream;
use async_nats::jetstream::stream::{self, RetentionPolicy, StorageType};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::Client as PgClient;

pub async fn connect(cfg: &AppConfig) -> Result<async_nats::Client> {
    Ok(async_nats::connect(&cfg.nats_url).await?)
}

pub async fn ensure_streams(client: async_nats::Client) -> Result<()> {
    let context = jetstream::new(client);
    let streams = [
        stream::Config {
            name: "AMI_INDEX".into(),
            subjects: vec!["ami.index.>".into()],
            retention: RetentionPolicy::WorkQueue,
            storage: StorageType::File,
            ..Default::default()
        },
        stream::Config {
            name: "AMI_RUNS".into(),
            subjects: vec!["ami.run.>".into()],
            retention: RetentionPolicy::Limits,
            storage: StorageType::File,
            ..Default::default()
        },
        stream::Config {
            name: "AMI_EVENTS".into(),
            subjects: vec!["ami.event.>".into()],
            retention: RetentionPolicy::Limits,
            storage: StorageType::File,
            ..Default::default()
        },
    ];
    for config in streams {
        context.get_or_create_stream(config).await?;
    }
    Ok(())
}

pub async fn status_stream_names(client: async_nats::Client) -> Result<Vec<String>> {
    let context = jetstream::new(client);
    let mut names = Vec::new();
    for stream_name in ["AMI_INDEX", "AMI_RUNS", "AMI_EVENTS"] {
        if context.get_stream(stream_name).await.is_ok() {
            names.push(stream_name.to_string());
        }
    }
    Ok(names)
}

pub async fn relay_memory_write_outbox(
    cfg: &AppConfig,
    db: &PgClient,
    limit: i64,
) -> Result<usize> {
    let client = connect(cfg).await?;
    let deliveries = postgres::claim_pending_memory_write_outbox(db, limit).await?;
    let mut published = 0usize;
    for delivery in deliveries {
        let payload = serde_json::to_vec(&delivery.payload)?;
        match client
            .publish(delivery.subject.clone(), payload.into())
            .await
        {
            Ok(_) => {
                client.flush().await?;
                let published_at_epoch_ms =
                    SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
                postgres::mark_memory_write_outbox_published(
                    db,
                    delivery.memory_write_outbox_id,
                    published_at_epoch_ms,
                )
                .await?;
                published += 1;
            }
            Err(error) => {
                postgres::mark_memory_write_outbox_failed(
                    db,
                    delivery.memory_write_outbox_id,
                    &error.to_string(),
                )
                .await?;
            }
        }
    }
    Ok(published)
}
