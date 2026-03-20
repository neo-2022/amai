use anyhow::Result;
use rusqlite::{Connection, params};
use std::fs;
use std::path::{Path, PathBuf};

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
