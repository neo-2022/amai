use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CachedContextPackEntry {
    pub context_pack_id: String,
    pub payload_json: String,
    pub durably_persisted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFastContextPackEntry {
    pub context_pack_id: String,
    pub payload_json: String,
    pub durably_persisted: bool,
    pub cached_at_epoch_ms: u128,
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
    fs::create_dir_all(fast_context_pack_sidecar_dir(path))?;
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
            snippet_preview TEXT,
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

        CREATE TABLE IF NOT EXISTS fast_context_pack_cache_entries (
            fast_cache_key TEXT PRIMARY KEY,
            context_pack_id TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            durably_persisted INTEGER NOT NULL DEFAULT 0,
            cached_at_epoch_ms TEXT NOT NULL
        );
        "#,
    )?;
    ensure_cached_documents_snippet_preview_column(&conn)?;
    Ok(path.to_path_buf())
}

fn ensure_cached_documents_snippet_preview_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(cached_documents)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut has_snippet_preview = false;
    for row in rows {
        if row? == "snippet_preview" {
            has_snippet_preview = true;
            break;
        }
    }
    if !has_snippet_preview {
        conn.execute(
            "ALTER TABLE cached_documents ADD COLUMN snippet_preview TEXT",
            [],
        )?;
        conn.execute(
            "UPDATE cached_documents SET snippet_preview = substr(content, 1, 2000) WHERE snippet_preview IS NULL",
            [],
        )?;
    }
    Ok(())
}

fn preview_text(content: &str, max_chars: usize) -> String {
    content.chars().take(max_chars).collect()
}

fn fast_context_pack_sidecar_dir(path: &Path) -> PathBuf {
    path.with_extension("").join("fast_context_pack_entries")
}

fn document_snippet_sidecar_dir(path: &Path) -> PathBuf {
    path.with_extension("").join("document_snippets")
}

fn document_snippet_sidecar_path(
    path: &Path,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(project_code.as_bytes());
    hasher.update([0]);
    hasher.update(namespace_code.as_bytes());
    hasher.update([0]);
    hasher.update(relative_path.as_bytes());
    let key = format!("{:x}", hasher.finalize());
    document_snippet_sidecar_dir(path).join(format!("{key}.txt"))
}

fn write_document_snippet_sidecar(
    path: &Path,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
    snippet_preview: &str,
) -> Result<()> {
    let sidecar_path =
        document_snippet_sidecar_path(path, project_code, namespace_code, relative_path);
    let parent = sidecar_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("document snippet sidecar path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    fs::write(&sidecar_path, snippet_preview.as_bytes())
        .with_context(|| format!("failed to write {}", sidecar_path.display()))?;
    Ok(())
}

fn read_document_snippet_sidecar(
    path: &Path,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
) -> Result<Option<String>> {
    let sidecar_path =
        document_snippet_sidecar_path(path, project_code, namespace_code, relative_path);
    let bytes = match fs::read(&sidecar_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read {}", sidecar_path.display()));
        }
    };
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn fast_context_pack_sidecar_path(path: &Path, fast_cache_key: u128) -> PathBuf {
    fast_context_pack_sidecar_dir(path).join(format!("{fast_cache_key}.json"))
}

fn write_fast_context_pack_sidecar(
    path: &Path,
    fast_cache_key: u128,
    entry: &CachedFastContextPackEntry,
) -> Result<()> {
    let sidecar_path = fast_context_pack_sidecar_path(path, fast_cache_key);
    let parent = sidecar_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("fast cache sidecar path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let temp_path = sidecar_path.with_extension("json.tmp");
    fs::write(&temp_path, serde_json::to_vec(entry)?)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, &sidecar_path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            temp_path.display(),
            sidecar_path.display()
        )
    })?;
    Ok(())
}

fn read_fast_context_pack_sidecar(
    path: &Path,
    fast_cache_key: u128,
) -> Result<Option<CachedFastContextPackEntry>> {
    let sidecar_path = fast_context_pack_sidecar_path(path, fast_cache_key);
    let bytes = match fs::read(&sidecar_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read {}", sidecar_path.display()));
        }
    };
    let entry = serde_json::from_slice::<CachedFastContextPackEntry>(&bytes)
        .with_context(|| format!("failed to decode {}", sidecar_path.display()))?;
    Ok(Some(entry))
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
    let snippet_preview = preview_text(content, 2000);
    conn.execute(
        "DELETE FROM cached_documents WHERE doc_key = ?1",
        params![doc_key],
    )?;
    conn.execute(
        "DELETE FROM cached_documents_fts WHERE doc_key = ?1",
        params![doc_key],
    )?;
    conn.execute(
        "INSERT INTO cached_documents(doc_key, project_code, namespace_code, relative_path, snippet_preview, content) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![doc_key, project_code, namespace_code, relative_path, snippet_preview, content],
    )?;
    write_document_snippet_sidecar(
        path,
        project_code,
        namespace_code,
        relative_path,
        &snippet_preview,
    )?;
    conn.execute(
        "INSERT INTO cached_documents_fts(doc_key, project_code, namespace_code, relative_path, content) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![doc_key, project_code, namespace_code, relative_path, content],
    )?;
    Ok(())
}

pub fn get_cached_document_by_path(
    path: &Path,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
) -> Result<Option<String>> {
    let conn = Connection::open(path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT content
        FROM cached_documents
        WHERE project_code = ?1
          AND namespace_code = ?2
          AND relative_path = ?3
        LIMIT 1
        "#,
    )?;
    let mut rows = stmt.query(params![project_code, namespace_code, relative_path])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(row.get(0)?))
}

pub fn get_cached_document_snippet_by_path(
    path: &Path,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
    max_bytes: usize,
) -> Result<Option<String>> {
    if let Some(snippet) =
        read_document_snippet_sidecar(path, project_code, namespace_code, relative_path)?
    {
        return Ok(Some(snippet.chars().take(max_bytes).collect()));
    }
    let conn = Connection::open(path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT COALESCE(snippet_preview, substr(content, 1, ?4))
        FROM cached_documents
        WHERE project_code = ?1
          AND namespace_code = ?2
          AND relative_path = ?3
        LIMIT 1
        "#,
    )?;
    let mut rows = stmt.query(params![
        project_code,
        namespace_code,
        relative_path,
        max_bytes as i64
    ])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(row.get(0)?))
}

pub fn get_cached_document_by_project_path(
    path: &Path,
    project_code: &str,
    relative_path: &str,
) -> Result<Option<String>> {
    let conn = Connection::open(path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT content
        FROM cached_documents
        WHERE project_code = ?1
          AND relative_path = ?2
        ORDER BY cached_at DESC
        LIMIT 1
        "#,
    )?;
    let mut rows = stmt.query(params![project_code, relative_path])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(row.get(0)?))
}

pub fn get_cached_document_snippet_by_project_path(
    path: &Path,
    project_code: &str,
    relative_path: &str,
    max_bytes: usize,
) -> Result<Option<String>> {
    let conn = Connection::open(path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT COALESCE(snippet_preview, substr(content, 1, ?3))
        FROM cached_documents
        WHERE project_code = ?1
          AND relative_path = ?2
        ORDER BY cached_at DESC
        LIMIT 1
        "#,
    )?;
    let mut rows = stmt.query(params![project_code, relative_path, max_bytes as i64])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(row.get(0)?))
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

pub fn upsert_fast_context_pack_cache_entry(
    path: &Path,
    fast_cache_key: u128,
    context_pack_id: &str,
    payload_json: &str,
    durably_persisted: bool,
    cached_at_epoch_ms: u128,
) -> Result<()> {
    let entry = CachedFastContextPackEntry {
        context_pack_id: context_pack_id.to_string(),
        payload_json: payload_json.to_string(),
        durably_persisted,
        cached_at_epoch_ms,
    };
    write_fast_context_pack_sidecar(path, fast_cache_key, &entry)?;
    let conn = Connection::open(path)?;
    conn.execute(
        r#"
        INSERT INTO fast_context_pack_cache_entries(
            fast_cache_key,
            context_pack_id,
            payload_json,
            durably_persisted,
            cached_at_epoch_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT (fast_cache_key) DO UPDATE SET
            context_pack_id = excluded.context_pack_id,
            payload_json = excluded.payload_json,
            durably_persisted = excluded.durably_persisted,
            cached_at_epoch_ms = excluded.cached_at_epoch_ms
        "#,
        params![
            fast_cache_key.to_string(),
            context_pack_id,
            payload_json,
            if durably_persisted { 1 } else { 0 },
            cached_at_epoch_ms.to_string()
        ],
    )?;
    Ok(())
}

pub fn get_fast_context_pack_cache_entry(
    path: &Path,
    fast_cache_key: u128,
) -> Result<Option<CachedFastContextPackEntry>> {
    if let Some(entry) = read_fast_context_pack_sidecar(path, fast_cache_key)? {
        return Ok(Some(entry));
    }
    let conn = Connection::open(path)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT context_pack_id, payload_json, durably_persisted, cached_at_epoch_ms
        FROM fast_context_pack_cache_entries
        WHERE fast_cache_key = ?1
        "#,
    )?;
    let mut rows = stmt.query(params![fast_cache_key.to_string()])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };

    let cached_at_epoch_ms = row.get::<_, String>(3)?.parse::<u128>()?;
    Ok(Some(CachedFastContextPackEntry {
        context_pack_id: row.get(0)?,
        payload_json: row.get(1)?,
        durably_persisted: row.get::<_, i64>(2)? != 0,
        cached_at_epoch_ms,
    }))
}
