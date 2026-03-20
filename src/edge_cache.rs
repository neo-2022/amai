use anyhow::Result;
use rusqlite::{Connection, params};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CachedContextPackEntry {
    pub context_pack_id: String,
    pub payload_json: String,
    pub durably_persisted: bool,
}

#[derive(Debug, Clone)]
pub struct ContextPackCacheRecord<'a> {
    pub cache_key: &'a str,
    pub scope_signature: &'a str,
    pub context_pack_id: &'a str,
    pub project_code: &'a str,
    pub namespace_code: &'a str,
    pub retrieval_mode: &'a str,
    pub payload_json: &'a str,
    pub durably_persisted: bool,
}

pub fn ensure(path: &Path) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS cached_documents (
            doc_key TEXT PRIMARY KEY,
            project_code TEXT NOT NULL,
            namespace_code TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            content TEXT NOT NULL,
            cached_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS cached_documents_fts USING fts5(
            doc_key,
            project_code,
            namespace_code,
            relative_path,
            content
        );

        CREATE TABLE IF NOT EXISTS context_packs (
            context_pack_id TEXT PRIMARY KEY,
            project_code TEXT NOT NULL,
            retrieval_mode TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            cached_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS context_pack_cache_entries (
            cache_key TEXT PRIMARY KEY,
            scope_signature TEXT NOT NULL,
            context_pack_id TEXT NOT NULL,
            project_code TEXT NOT NULL,
            namespace_code TEXT NOT NULL,
            retrieval_mode TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            durably_persisted INTEGER NOT NULL DEFAULT 0,
            cached_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )?;
    Ok(path.to_path_buf())
}

pub fn upsert_document(
    path: &Path,
    doc_key: &str,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
    content: &str,
) -> Result<()> {
    let conn = Connection::open(path)?;
    conn.execute(
        "DELETE FROM cached_documents WHERE doc_key = ?1",
        params![doc_key],
    )?;
    conn.execute(
        "DELETE FROM cached_documents_fts WHERE doc_key = ?1",
        params![doc_key],
    )?;
    conn.execute(
        "INSERT INTO cached_documents(doc_key, project_code, namespace_code, relative_path, content) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![doc_key, project_code, namespace_code, relative_path, content],
    )?;
    conn.execute(
        "INSERT INTO cached_documents_fts(doc_key, project_code, namespace_code, relative_path, content) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![doc_key, project_code, namespace_code, relative_path, content],
    )?;
    Ok(())
}

pub fn cache_context_pack(
    path: &Path,
    context_pack_id: &str,
    project_code: &str,
    retrieval_mode: &str,
    payload_json: &str,
) -> Result<()> {
    let conn = Connection::open(path)?;
    conn.execute(
        "DELETE FROM context_packs WHERE context_pack_id = ?1",
        params![context_pack_id],
    )?;
    conn.execute(
        "INSERT INTO context_packs(context_pack_id, project_code, retrieval_mode, payload_json) VALUES (?1, ?2, ?3, ?4)",
        params![context_pack_id, project_code, retrieval_mode, payload_json],
    )?;
    Ok(())
}

pub fn upsert_context_pack_cache_entry(
    path: &Path,
    record: &ContextPackCacheRecord<'_>,
) -> Result<()> {
    let conn = Connection::open(path)?;
    conn.execute(
        r#"
        INSERT INTO context_pack_cache_entries(
            cache_key,
            scope_signature,
            context_pack_id,
            project_code,
            namespace_code,
            retrieval_mode,
            payload_json,
            durably_persisted
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT (cache_key) DO UPDATE SET
            scope_signature = excluded.scope_signature,
            context_pack_id = excluded.context_pack_id,
            project_code = excluded.project_code,
            namespace_code = excluded.namespace_code,
            retrieval_mode = excluded.retrieval_mode,
            payload_json = excluded.payload_json,
            durably_persisted = excluded.durably_persisted,
            cached_at = CURRENT_TIMESTAMP
        "#,
        params![
            record.cache_key,
            record.scope_signature,
            record.context_pack_id,
            record.project_code,
            record.namespace_code,
            record.retrieval_mode,
            record.payload_json,
            if record.durably_persisted { 1 } else { 0 }
        ],
    )?;
    Ok(())
}

pub fn get_context_pack_cache_entry(
    path: &Path,
    cache_key: &str,
    scope_signature: &str,
) -> Result<Option<CachedContextPackEntry>> {
    let conn = Connection::open(path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT context_pack_id, payload_json, durably_persisted
        FROM context_pack_cache_entries
        WHERE cache_key = ?1 AND scope_signature = ?2
        "#,
    )?;
    let mut rows = stmt.query(params![cache_key, scope_signature])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };

    Ok(Some(CachedContextPackEntry {
        context_pack_id: row.get(0)?,
        payload_json: row.get(1)?,
        durably_persisted: row.get::<_, i64>(2)? != 0,
    }))
}
