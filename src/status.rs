use crate::{compatibility, config::AppConfig, nats, postgres, qdrant, s3};
use anyhow::{Context, Result};
use reqwest::StatusCode;
use std::time::Duration;

pub async fn print_status(cfg: &AppConfig) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    let (projects, namespaces, documents) = postgres::status_counts(&db).await?;
    let app_db = postgres::connect_app(cfg).await?;
    let (app_projects, app_namespaces, app_documents) = postgres::status_counts(&app_db).await?;

    let qdrant_client = qdrant::connect(cfg)?;
    let code_exists = qdrant_client
        .collection_exists(&cfg.qdrant_collection_code)
        .await?;
    let memory_exists = qdrant_client
        .collection_exists(&cfg.qdrant_collection_memory)
        .await?;

    let s3_client = s3::connect(cfg).await?;
    let buckets = s3::status_bucket_names(&s3_client).await?;

    let nats_client = nats::connect(cfg).await?;
    let streams = nats::status_stream_names(nats_client).await?;

    let nats_http = http_client()?.get(&cfg.nats_http_url).send().await?;
    let nats_http_ok = nats_http.status() == StatusCode::OK;

    println!("stack: {}", cfg.stack_name);
    println!(
        "postgres: ok (admin_projects={projects}, admin_namespaces={namespaces}, admin_documents={documents}, app_projects={app_projects}, app_namespaces={app_namespaces}, app_documents={app_documents})"
    );
    println!(
        "qdrant: ok (code_collection={}, memory_collection={})",
        code_exists, memory_exists
    );
    println!("s3: ok (buckets={})", buckets.join(", "));
    println!(
        "nats: ok (http={}, streams={})",
        nats_http_ok,
        streams.join(", ")
    );
    println!("memory_embed_model: {}", cfg.memory_embed_model);
    println!("edge_cache: {}", cfg.edge_cache_path.display());
    let compatibility = compatibility::check(cfg).await?;
    println!(
        "compatibility: {} (profile={}, postgres={}, qdrant={}, nats={}, s3={})",
        if compatibility.compatible() {
            "ok"
        } else {
            "FAIL"
        },
        compatibility.profile,
        compatibility.postgres.raw_version,
        compatibility.qdrant.raw_version,
        compatibility.nats.raw_version,
        compatibility.s3.raw_version
    );
    Ok(())
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .context("failed to build status HTTP client")
}
