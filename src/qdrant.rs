use crate::config::AppConfig;
use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    Condition, CreateAliasBuilder, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter,
    PointStruct, ScoredPoint, SearchPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct VectorPoint {
    pub point_id: Uuid,
    pub vector: Vec<f32>,
    pub payload: Value,
}

pub fn connect(cfg: &AppConfig) -> Result<Qdrant> {
    Qdrant::from_url(&cfg.qdrant_url)
        .build()
        .with_context(|| format!("failed to connect to Qdrant at {}", cfg.qdrant_url))
}

pub async fn bootstrap_collections(client: &Qdrant, cfg: &AppConfig) -> Result<()> {
    ensure_collection_and_alias(
        client,
        &cfg.qdrant_collection_code,
        &cfg.qdrant_alias_code,
        cfg.qdrant_code_dim,
        cfg.qdrant_distance()?,
    )
    .await?;
    ensure_collection_and_alias(
        client,
        &cfg.qdrant_collection_memory,
        &cfg.qdrant_alias_memory,
        cfg.qdrant_memory_dim,
        cfg.qdrant_distance()?,
    )
    .await?;
    Ok(())
}

async fn ensure_collection_and_alias(
    client: &Qdrant,
    collection: &str,
    alias: &str,
    size: u64,
    distance: Distance,
) -> Result<()> {
    if !client.collection_exists(collection).await? {
        client
            .create_collection(
                CreateCollectionBuilder::new(collection)
                    .vectors_config(VectorParamsBuilder::new(size, distance)),
            )
            .await
            .with_context(|| format!("failed to create collection {collection}"))?;
    }

    let aliases = client.list_aliases().await?;
    let alias_present = aliases
        .aliases
        .iter()
        .any(|entry| entry.alias_name == alias && entry.collection_name == collection);
    if !alias_present {
        let alias_exists_elsewhere = aliases
            .aliases
            .iter()
            .any(|entry| entry.alias_name == alias);
        if alias_exists_elsewhere {
            client.delete_alias(alias).await?;
        }
        client
            .create_alias(CreateAliasBuilder::new(collection, alias))
            .await
            .with_context(|| format!("failed to create alias {alias} -> {collection}"))?;
    }
    Ok(())
}

pub async fn replace_document_points(
    client: &Qdrant,
    collection_alias: &str,
    document_id: Uuid,
    points: &[VectorPoint],
) -> Result<()> {
    client
        .delete_points(
            DeletePointsBuilder::new(collection_alias).points(Filter::all([Condition::matches(
                "document_id",
                document_id.to_string(),
            )])),
        )
        .await
        .with_context(|| format!("failed to clear stale points for document {document_id}"))?;

    if points.is_empty() {
        return Ok(());
    }

    let payload_points = points
        .iter()
        .map(|point| -> Result<PointStruct> {
            let payload: Payload = point.payload.clone().try_into()?;
            Ok(PointStruct::new(
                point.point_id.to_string(),
                point.vector.clone(),
                payload,
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    client
        .upsert_points(UpsertPointsBuilder::new(collection_alias, payload_points))
        .await
        .with_context(|| format!("failed to upsert points into {collection_alias}"))?;
    Ok(())
}

pub async fn search_project_points(
    client: &Qdrant,
    collection_alias: &str,
    vector: Vec<f32>,
    project_code: &str,
    limit: usize,
) -> Result<Vec<ScoredPoint>> {
    let response = client
        .search_points(
            SearchPointsBuilder::new(collection_alias, vector, limit as u64)
                .filter(Filter::must([Condition::matches(
                    "project_code",
                    project_code.to_string(),
                )]))
                .with_payload(true),
        )
        .await
        .with_context(|| {
            format!("failed semantic search in {collection_alias} for project {project_code}")
        })?;
    Ok(response.result)
}
