use crate::cli::{ContextPackArgs, WarmupCacheArgs};
use crate::config::AppConfig;
use crate::retrieval;
use anyhow::{Result, anyhow};
use serde_json::json;
use tokio_postgres::Client;

pub async fn run(cfg: &AppConfig, db: &mut Client, args: &WarmupCacheArgs) -> Result<()> {
    if args.projects.is_empty() {
        return Err(anyhow!("warmup requires at least one project"));
    }

    let mut warmed = Vec::with_capacity(args.projects.len());
    for project in &args.projects {
        let context = ContextPackArgs {
            project: project.clone(),
            namespace: args.namespace.clone(),
            query: args.query.clone(),
            retrieval_mode: args.retrieval_mode.clone(),
            disable_cache: false,
            limit_documents: args.limit_documents,
            limit_symbols: args.limit_symbols,
            limit_chunks: args.limit_chunks,
            limit_semantic_chunks: args.limit_semantic_chunks,
            token_source_kind: "proof_warmup_context_pack".to_string(),
        };
        let stats =
            retrieval::execute_context_pack_with_options(cfg, db, &context, true, false).await?;
        warmed.push(json!({
            "project": project,
            "namespace": args.namespace,
            "query": args.query,
            "cache_hit": stats.cache_hit,
            "exact_documents": stats.exact_documents,
            "symbol_hits": stats.symbol_hits,
            "lexical_chunks": stats.lexical_chunks,
            "semantic_chunks": stats.semantic_chunks,
            "scope_signature": stats.scope_signature
        }));
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "warmup_cache": {
                "projects": args.projects,
                "namespace": args.namespace,
                "query": args.query,
                "retrieval_mode": args.retrieval_mode,
                "warmed": warmed
            }
        }))?
    );

    Ok(())
}
