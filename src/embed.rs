// ponytail: tiny standalone embedder surface so skill ranking doesn't pull in the whole retrieval module
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use anyhow::{Result, anyhow};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use crate::config::AppConfig;

struct CachedEmbedder {
    model: String,
    embedder: TextEmbedding,
    query_cache: HashMap<String, Vec<f32>>,
}

static EMBEDDER_CACHE: OnceLock<Mutex<Option<CachedEmbedder>>> = OnceLock::new();

fn build_embedder(cfg: &AppConfig) -> Result<TextEmbedding> {
    let model = match cfg.code_embed_model.as_str() {
        "jina_base_code" => EmbeddingModel::JinaEmbeddingsV2BaseCode,
        "multilingual_e5_small" => EmbeddingModel::MultilingualE5Small,
        "multilingual_e5_base" => EmbeddingModel::MultilingualE5Base,
        other => return Err(anyhow!("unsupported code embedding model: {other}")),
    };
    TextEmbedding::try_new(InitOptions::new(model).with_show_download_progress(false))
}

pub(crate) fn embed_text(cfg: &AppConfig, text: &str) -> Result<(Vec<f32>, u128)> {
    let (vectors, duration) = embed_text_batch(cfg, &[text.to_string()])?;
    let vector = vectors
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("semantic embedder returned no vector"))?;
    Ok((vector, duration))
}

pub(crate) fn embed_text_batch(
    cfg: &AppConfig,
    texts: &[String],
) -> Result<(Vec<Vec<f32>>, u128)> {
    let started = Instant::now();
    let cache = EMBEDDER_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow!("embedder cache lock poisoned"))?;
    let needs_rebuild = guard
        .as_ref()
        .map(|cached| cached.model != cfg.code_embed_model)
        .unwrap_or(true);
    if needs_rebuild {
        *guard = Some(CachedEmbedder {
            model: cfg.code_embed_model.clone(),
            embedder: build_embedder(cfg)?,
            query_cache: HashMap::new(),
        });
    }
    let cached = guard
        .as_mut()
        .ok_or_else(|| anyhow!("embedder cache unexpectedly empty"))?;

    let mut result: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
    let mut batch_inputs: Vec<String> = Vec::new();
    let mut batch_index: Vec<usize> = Vec::new();
    for (index, text) in texts.iter().enumerate() {
        if let Some(vector) = cached.query_cache.get(text).cloned() {
            result[index] = Some(vector);
        } else {
            batch_inputs.push(text.clone());
            batch_index.push(index);
        }
    }

    if !batch_inputs.is_empty() {
        let embeddings = cached.embedder.embed(&batch_inputs, None)?;
        for (offset, (text, vector)) in batch_inputs.iter().zip(embeddings.into_iter()).enumerate() {
            let index = batch_index[offset];
            result[index] = Some(vector.clone());
            if cached.query_cache.len() >= 384 {
                if let Some(oldest_key) = cached.query_cache.keys().next().cloned() {
                    cached.query_cache.remove(&oldest_key);
                }
            }
            cached.query_cache.insert(text.clone(), vector);
        }
    }

    let vectors = result
        .into_iter()
        .map(|maybe| maybe.expect("semantic embedder missing vector for batched input"))
        .collect();
    Ok((vectors, started.elapsed().as_millis()))
}

pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
