use crate::config::AppConfig;
use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    Condition, CreateAliasBuilder, CreateCollectionBuilder, CreateFieldIndexCollectionBuilder,
    DeletePointsBuilder, Distance, FieldType, Filter, PayloadSchemaType, PointStruct,
    RetrievedPoint, ScoredPoint, ScrollPointsBuilder, SearchPointsBuilder, UpsertPointsBuilder,
    VectorParamsBuilder, WriteOrdering, WriteOrderingType, point_id::PointIdOptions,
    vector_output::Vector as VectorOutputKind,
};
use qdrant_client::{Payload, Qdrant};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct VectorPoint {
    pub point_id: Uuid,
    pub vector: Vec<f32>,
    pub payload: Value,
}

#[derive(Clone)]
struct CachedQdrantClient {
    qdrant_url: String,
    client: Qdrant,
}

static QDRANT_CLIENT_CACHE: OnceLock<Mutex<Option<CachedQdrantClient>>> = OnceLock::new();

fn strong_write_ordering() -> WriteOrdering {
    WriteOrdering {
        r#type: WriteOrderingType::Strong as i32,
    }
}

pub fn connect(cfg: &AppConfig) -> Result<Qdrant> {
    let cache = QDRANT_CLIENT_CACHE.get_or_init(|| Mutex::new(None));
    if let Some(cached) = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("qdrant client cache lock poisoned"))?
        .as_ref()
        .filter(|cached| cached.qdrant_url == cfg.qdrant_url)
    {
        return Ok(cached.client.clone());
    }

    let client = Qdrant::from_url(&cfg.qdrant_url)
        .build()
        .with_context(|| format!("failed to connect to Qdrant at {}", cfg.qdrant_url))?;
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("qdrant client cache lock poisoned"))?;
    *guard = Some(CachedQdrantClient {
        qdrant_url: cfg.qdrant_url.clone(),
        client: client.clone(),
    });
    Ok(client)
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
    ensure_code_collection_payload_indexes(client, &cfg.qdrant_collection_code).await?;
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

async fn ensure_code_collection_payload_indexes(client: &Qdrant, collection: &str) -> Result<()> {
    ensure_keyword_field_index(client, collection, "project_code").await?;
    ensure_keyword_field_index(client, collection, "namespace_code").await?;
    ensure_keyword_field_index(client, collection, "document_id").await?;
    Ok(())
}

async fn ensure_keyword_field_index(
    client: &Qdrant,
    collection: &str,
    field_name: &str,
) -> Result<()> {
    match client
        .create_field_index(CreateFieldIndexCollectionBuilder::new(
            collection,
            field_name,
            FieldType::Keyword,
        ))
        .await
    {
        Ok(_) => {}
        Err(error) => {
            if collection_has_keyword_payload_index(client, collection, field_name).await? {
                tracing::warn!(
                    collection,
                    field_name,
                    error = %error,
                    "qdrant returned an index creation error after the keyword payload index was already observable; accepting verified state"
                );
                return Ok(());
            }
            return Err(error).with_context(|| {
                format!("failed to ensure payload index {field_name} in {collection}")
            });
        }
    }
    Ok(())
}

async fn collection_has_keyword_payload_index(
    client: &Qdrant,
    collection: &str,
    field_name: &str,
) -> Result<bool> {
    let info = client.collection_info(collection).await.with_context(|| {
        format!("failed to inspect collection {collection} after payload index create error")
    })?;
    Ok(info
        .result
        .as_ref()
        .and_then(|result| result.payload_schema.get(field_name))
        .map(|schema| schema.data_type == PayloadSchemaType::Keyword as i32)
        .unwrap_or(false))
}

pub async fn replace_document_points(
    client: &Qdrant,
    collection_alias: &str,
    document_id: Uuid,
    points: &[VectorPoint],
) -> Result<()> {
    replace_document_points_with_prior_snapshot(client, collection_alias, document_id, points)
        .await
        .map(|_| ())
}

pub async fn replace_document_points_with_prior_snapshot(
    client: &Qdrant,
    collection_alias: &str,
    document_id: Uuid,
    points: &[VectorPoint],
) -> Result<Vec<VectorPoint>> {
    let existing_points = snapshot_document_points(client, collection_alias, document_id).await?;
    if let Err(clear_error) = clear_document_points(client, collection_alias, document_id).await {
        restore_document_points(client, collection_alias, &existing_points)
            .await
            .with_context(|| {
                format!(
                    "failed to restore prior qdrant points for document {document_id} after clear-before-upsert failure"
                )
            })
            .map_err(|restore_error| {
                anyhow::anyhow!(
                    "{clear_error:#}\nsecondary qdrant restore failure after clear-before-upsert path: {restore_error:#}"
                )
            })?;
        return Err(clear_error);
    }

    if points.is_empty() {
        return Ok(existing_points);
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

    match client
        .upsert_points(
            UpsertPointsBuilder::new(collection_alias, payload_points)
                .wait(true)
                .ordering(strong_write_ordering()),
        )
        .await
        .with_context(|| format!("failed to upsert points into {collection_alias}"))
    {
        Ok(_) => {}
        Err(upsert_error) => {
            clear_document_points(client, collection_alias, document_id)
                .await
                .with_context(|| {
                    format!(
                        "failed to clear partially written replacement points for document {document_id} after upsert failure"
                    )
                })
                .map_err(|clear_error| {
                    anyhow::anyhow!(
                        "{upsert_error:#}\nsecondary qdrant clear failure before restore path: {clear_error:#}"
                    )
                })?;
            restore_document_points(client, collection_alias, &existing_points)
                .await
                .with_context(|| {
                    format!(
                        "failed to restore prior qdrant points for document {document_id} after upsert failure"
                    )
                })
                .map_err(|restore_error| {
                    anyhow::anyhow!(
                        "{upsert_error:#}\nsecondary qdrant restore failure after clear-before-upsert path: {restore_error:#}"
                    )
                })?;
            return Err(upsert_error);
        }
    }
    Ok(existing_points)
}

async fn snapshot_document_points(
    client: &Qdrant,
    collection_alias: &str,
    document_id: Uuid,
) -> Result<Vec<VectorPoint>> {
    let mut points = Vec::new();
    let mut offset = None;
    loop {
        let mut builder = ScrollPointsBuilder::new(collection_alias)
            .filter(Filter::all([Condition::matches(
                "document_id",
                document_id.to_string(),
            )]))
            .limit(256)
            .with_payload(true)
            .with_vectors(true);
        if let Some(offset_value) = offset.clone() {
            builder = builder.offset(offset_value);
        }
        let response = client.scroll(builder).await.with_context(|| {
            format!("failed to snapshot existing points for document {document_id}")
        })?;
        for point in response.result {
            points.push(vector_point_from_retrieved_point(point)?);
        }
        if let Some(next_offset) = response.next_page_offset {
            offset = Some(next_offset);
        } else {
            break;
        }
    }
    Ok(points)
}

#[cfg(test)]
pub(crate) async fn snapshot_document_points_for_tests(
    client: &Qdrant,
    collection_alias: &str,
    document_id: Uuid,
) -> Result<Vec<VectorPoint>> {
    snapshot_document_points(client, collection_alias, document_id).await
}

async fn restore_document_points(
    client: &Qdrant,
    collection_alias: &str,
    points: &[VectorPoint],
) -> Result<()> {
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
        .upsert_points(
            UpsertPointsBuilder::new(collection_alias, payload_points)
                .wait(true)
                .ordering(strong_write_ordering()),
        )
        .await
        .with_context(|| format!("failed to restore points into {collection_alias}"))?;
    Ok(())
}

fn vector_point_from_retrieved_point(point: RetrievedPoint) -> Result<VectorPoint> {
    let point_id = match point.id.and_then(|id| id.point_id_options) {
        Some(PointIdOptions::Uuid(value)) => {
            Uuid::parse_str(&value).with_context(|| format!("invalid qdrant point uuid {value}"))?
        }
        Some(PointIdOptions::Num(value)) => {
            return Err(anyhow::anyhow!(
                "numeric qdrant point id {value} is unsupported for vector restore"
            ));
        }
        None => {
            return Err(anyhow::anyhow!(
                "retrieved qdrant point missing id for vector restore"
            ));
        }
    };
    let vectors = point
        .vectors
        .ok_or_else(|| anyhow::anyhow!("retrieved qdrant point {point_id} missing vectors"))?;
    let vector = match vectors.get_vector() {
        Some(VectorOutputKind::Dense(vector)) => vector.data,
        Some(_) => {
            return Err(anyhow::anyhow!(
                "retrieved qdrant point {point_id} uses unsupported non-dense vectors"
            ));
        }
        None => {
            return Err(anyhow::anyhow!(
                "retrieved qdrant point {point_id} missing default vector payload"
            ));
        }
    };
    let payload_json: serde_json::Value = Payload::from(point.payload).into();
    Ok(VectorPoint {
        point_id,
        vector,
        payload: payload_json,
    })
}

pub async fn clear_document_points(
    client: &Qdrant,
    collection_alias: &str,
    document_id: Uuid,
) -> Result<()> {
    client
        .delete_points(
            DeletePointsBuilder::new(collection_alias)
                .wait(true)
                .ordering(strong_write_ordering())
                .points(Filter::all([Condition::matches(
                    "document_id",
                    document_id.to_string(),
                )])),
        )
        .await
        .with_context(|| format!("failed to clear stale points for document {document_id}"))?;
    Ok(())
}

pub async fn search_namespace_points(
    client: &Qdrant,
    collection_alias: &str,
    vector: Vec<f32>,
    project_code: &str,
    namespace_code: &str,
    limit: usize,
) -> Result<Vec<ScoredPoint>> {
    let response = client
        .search_points(
            SearchPointsBuilder::new(collection_alias, vector, limit as u64)
                .filter(Filter::must([
                    Condition::matches("project_code", project_code.to_string()),
                    Condition::matches("namespace_code", namespace_code.to_string()),
                ]))
                .with_payload(true),
        )
        .await
        .with_context(|| {
            format!(
                "failed semantic search in {collection_alias} for project {project_code} namespace {namespace_code}"
            )
        })?;
    Ok(response.result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qdrant_client::qdrant::{
        DenseVector, PayloadSchemaInfo, PointId, RetrievedPoint, Value as QdrantValue,
        VectorsOutput, point_id::PointIdOptions, vectors_output,
    };
    use std::collections::HashMap;

    #[allow(deprecated)]
    #[test]
    fn vector_point_from_retrieved_point_round_trips_dense_payload() {
        let mut payload = HashMap::<String, QdrantValue>::new();
        payload.insert(
            "document_id".to_string(),
            QdrantValue::from("00000000-0000-0000-0000-000000000000"),
        );
        let point = RetrievedPoint {
            id: Some(PointId {
                point_id_options: Some(PointIdOptions::Uuid(Uuid::nil().to_string())),
            }),
            payload,
            vectors: Some(VectorsOutput {
                vectors_options: Some(vectors_output::VectorsOptions::Vector(
                    qdrant_client::qdrant::VectorOutput {
                        data: Vec::new(),
                        indices: None,
                        vectors_count: None,
                        vector: Some(VectorOutputKind::Dense(DenseVector {
                            data: vec![1.0, 2.0, 3.0],
                        })),
                    },
                )),
            }),
            shard_key: None,
            order_value: None,
        };

        let restored = vector_point_from_retrieved_point(point).expect("restore vector point");
        assert_eq!(restored.point_id, Uuid::nil());
        assert_eq!(restored.vector, vec![1.0, 2.0, 3.0]);
        assert_eq!(
            restored.payload["document_id"].as_str(),
            Some("00000000-0000-0000-0000-000000000000")
        );
    }

    #[test]
    fn vector_point_from_retrieved_point_rejects_missing_vectors() {
        let point = RetrievedPoint {
            id: Some(PointId {
                point_id_options: Some(PointIdOptions::Uuid(Uuid::nil().to_string())),
            }),
            payload: HashMap::new(),
            vectors: None,
            shard_key: None,
            order_value: None,
        };

        let error = vector_point_from_retrieved_point(point).expect_err("must reject");
        assert!(format!("{error:#}").contains("missing vectors"));
    }

    #[allow(deprecated)]
    #[test]
    fn vector_point_from_retrieved_point_rejects_non_dense_vectors() {
        let point = RetrievedPoint {
            id: Some(PointId {
                point_id_options: Some(PointIdOptions::Uuid(Uuid::nil().to_string())),
            }),
            payload: HashMap::new(),
            vectors: Some(VectorsOutput {
                vectors_options: Some(vectors_output::VectorsOptions::Vector(
                    qdrant_client::qdrant::VectorOutput {
                        data: Vec::new(),
                        indices: None,
                        vectors_count: None,
                        vector: Some(VectorOutputKind::MultiDense(
                            qdrant_client::qdrant::MultiDenseVector {
                                vectors: vec![DenseVector {
                                    data: vec![1.0, 2.0],
                                }],
                            },
                        )),
                    },
                )),
            }),
            shard_key: None,
            order_value: None,
        };

        let error = vector_point_from_retrieved_point(point).expect_err("must reject");
        assert!(format!("{error:#}").contains("unsupported non-dense vectors"));
    }

    #[test]
    fn keyword_payload_index_detection_is_exact() {
        let mut payload_schema = HashMap::new();
        payload_schema.insert(
            "project_code".to_string(),
            PayloadSchemaInfo {
                data_type: PayloadSchemaType::Keyword as i32,
                params: None,
                points: Some(0),
            },
        );
        payload_schema.insert(
            "document_id".to_string(),
            PayloadSchemaInfo {
                data_type: PayloadSchemaType::Integer as i32,
                params: None,
                points: Some(0),
            },
        );

        assert!(
            payload_schema
                .get("project_code")
                .map(|schema| schema.data_type == PayloadSchemaType::Keyword as i32)
                .unwrap_or(false)
        );
        assert!(
            !payload_schema
                .get("document_id")
                .map(|schema| schema.data_type == PayloadSchemaType::Keyword as i32)
                .unwrap_or(false)
        );
        assert!(
            !payload_schema
                .get("missing")
                .map(|schema| schema.data_type == PayloadSchemaType::Keyword as i32)
                .unwrap_or(false)
        );
    }
}
