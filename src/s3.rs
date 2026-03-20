use crate::config::AppConfig;
use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Builder, Region};
use aws_sdk_s3::primitives::ByteStream;

pub async fn connect(cfg: &AppConfig) -> Result<Client> {
    let shared = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(cfg.s3_region.clone()))
        .endpoint_url(cfg.s3_endpoint.clone())
        .credentials_provider(Credentials::new(
            cfg.s3_access_key.clone(),
            cfg.s3_secret_key.clone(),
            None,
            None,
            "amai",
        ))
        .load()
        .await;
    let config = Builder::from(&shared).force_path_style(true).build();
    Ok(Client::from_conf(config))
}

pub async fn ensure_buckets(client: &Client, cfg: &AppConfig) -> Result<()> {
    for bucket in [
        &cfg.s3_bucket_artifacts,
        &cfg.s3_bucket_transcripts,
        &cfg.s3_bucket_context,
    ] {
        let exists = client.head_bucket().bucket(bucket).send().await.is_ok();
        if !exists {
            client
                .create_bucket()
                .bucket(bucket)
                .send()
                .await
                .with_context(|| format!("failed to create bucket {bucket}"))?;
        }
    }
    Ok(())
}

pub async fn status_bucket_names(client: &Client) -> Result<Vec<String>> {
    let buckets = client.list_buckets().send().await?;
    Ok(buckets
        .buckets()
        .iter()
        .filter_map(|bucket| bucket.name().map(ToOwned::to_owned))
        .collect())
}

pub async fn put_json_object(
    client: &Client,
    bucket: &str,
    object_key: &str,
    body: &str,
) -> Result<()> {
    client
        .put_object()
        .bucket(bucket)
        .key(object_key)
        .content_type("application/json")
        .body(ByteStream::from(body.as_bytes().to_vec()))
        .send()
        .await
        .with_context(|| format!("failed to put {bucket}/{object_key}"))?;
    Ok(())
}
