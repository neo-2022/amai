use anyhow::{Context, Result, anyhow};
use qdrant_client::qdrant::Distance;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub stack_name: String,
    pub pg_db: String,
    pub app_db_user: String,
    pub app_db_password: String,
    pub postgres_dsn: String,
    pub app_postgres_dsn: String,
    pub qdrant_url: String,
    pub qdrant_http_url: String,
    pub qdrant_collection_code: String,
    pub qdrant_alias_code: String,
    pub qdrant_collection_memory: String,
    pub qdrant_alias_memory: String,
    pub qdrant_code_dim: u64,
    pub qdrant_memory_dim: u64,
    pub qdrant_distance: String,
    pub s3_endpoint: String,
    pub s3_region: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_bucket_artifacts: String,
    pub s3_bucket_transcripts: String,
    pub s3_bucket_context: String,
    pub nats_url: String,
    pub nats_http_url: String,
    pub code_embed_model: String,
    pub memory_embed_model: String,
    pub chunk_max_bytes: usize,
    pub fallback_chunk_lines: usize,
    pub fallback_chunk_overlap_lines: usize,
    pub edge_cache_path: PathBuf,
    pub default_retrieval_mode: String,
    pub local_fast_cache_ttl_ms: u128,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            stack_name: required("AMI_STACK_NAME")?,
            pg_db: required("AMI_PG_DB")?,
            app_db_user: required("AMI_APP_DB_USER")?,
            app_db_password: required("AMI_APP_DB_PASSWORD")?,
            postgres_dsn: required("AMI_POSTGRES_DSN")?,
            app_postgres_dsn: required("AMI_APP_POSTGRES_DSN")?,
            qdrant_url: required("AMI_QDRANT_URL")?,
            qdrant_http_url: required("AMI_QDRANT_HTTP_URL")?,
            qdrant_collection_code: required("AMI_QDRANT_COLLECTION_CODE")?,
            qdrant_alias_code: required("AMI_QDRANT_ALIAS_CODE")?,
            qdrant_collection_memory: required("AMI_QDRANT_COLLECTION_MEMORY")?,
            qdrant_alias_memory: required("AMI_QDRANT_ALIAS_MEMORY")?,
            qdrant_code_dim: required("AMI_QDRANT_CODE_DIM")?
                .parse()
                .context("AMI_QDRANT_CODE_DIM must be u64")?,
            qdrant_memory_dim: required("AMI_QDRANT_MEMORY_DIM")?
                .parse()
                .context("AMI_QDRANT_MEMORY_DIM must be u64")?,
            qdrant_distance: required("AMI_QDRANT_DISTANCE")?,
            s3_endpoint: required("AMI_S3_ENDPOINT")?,
            s3_region: required("AMI_S3_REGION")?,
            s3_access_key: required("AMI_S3_ACCESS_KEY")?,
            s3_secret_key: required("AMI_S3_SECRET_KEY")?,
            s3_bucket_artifacts: required("AMI_S3_BUCKET_ARTIFACTS")?,
            s3_bucket_transcripts: required("AMI_S3_BUCKET_TRANSCRIPTS")?,
            s3_bucket_context: required("AMI_S3_BUCKET_CONTEXT")?,
            nats_url: required("AMI_NATS_URL")?,
            nats_http_url: required("AMI_NATS_HTTP_URL")?,
            code_embed_model: required("AMI_CODE_EMBED_MODEL")?,
            memory_embed_model: required("AMI_MEMORY_EMBED_MODEL")?,
            chunk_max_bytes: required("AMI_CHUNK_MAX_BYTES")?
                .parse()
                .context("AMI_CHUNK_MAX_BYTES must be usize")?,
            fallback_chunk_lines: required("AMI_FALLBACK_CHUNK_LINES")?
                .parse()
                .context("AMI_FALLBACK_CHUNK_LINES must be usize")?,
            fallback_chunk_overlap_lines: required("AMI_FALLBACK_CHUNK_OVERLAP_LINES")?
                .parse()
                .context("AMI_FALLBACK_CHUNK_OVERLAP_LINES must be usize")?,
            edge_cache_path: PathBuf::from(required("AMI_EDGE_CACHE_PATH")?),
            default_retrieval_mode: required("AMI_DEFAULT_RETRIEVAL_MODE")?,
            local_fast_cache_ttl_ms: required("AMI_LOCAL_FAST_CACHE_TTL_MS")?
                .parse()
                .context("AMI_LOCAL_FAST_CACHE_TTL_MS must be u128")?,
        })
    }

    pub fn qdrant_distance(&self) -> Result<Distance> {
        match self.qdrant_distance.as_str() {
            "cosine" | "Cosine" => Ok(Distance::Cosine),
            "dot" | "Dot" => Ok(Distance::Dot),
            "euclid" | "Euclid" => Ok(Distance::Euclid),
            "manhattan" | "Manhattan" => Ok(Distance::Manhattan),
            other => Err(anyhow!("unsupported Qdrant distance: {other}")),
        }
    }
}

fn required(key: &str) -> Result<String> {
    env::var(key).with_context(|| format!("missing environment variable {key}"))
}
