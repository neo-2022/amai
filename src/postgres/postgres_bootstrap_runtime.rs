use super::*;

pub async fn connect_admin(cfg: &AppConfig) -> Result<Client> {
    connect(&cfg.postgres_dsn).await
}

pub async fn connect_app(cfg: &AppConfig) -> Result<Client> {
    connect(&cfg.app_postgres_dsn).await
}

async fn connect(dsn: &str) -> Result<Client> {
    let config: PostgresConfig = dsn
        .parse()
        .with_context(|| format!("invalid postgres dsn {}", safe_postgres_descriptor(dsn)))?;
    let masked_descriptor = safe_postgres_descriptor_from_config(&config);
    let ssl_mode = config.get_ssl_mode();
    match ssl_mode {
        SslMode::Disable => {
            let (client, connection) = config.connect(NoTls).await.with_context(|| {
                format!("failed to connect to postgres via {masked_descriptor}")
            })?;
            tokio::spawn(async move {
                if let Err(error) = connection.await {
                    tracing::error!(?error, "postgres connection task ended with error");
                }
            });
            Ok(client)
        }
        _ => {
            let connector = build_tls_connector().with_context(|| {
                format!("failed to initialize postgres TLS for {masked_descriptor}")
            })?;
            let (client, connection) = config.connect(connector).await.with_context(|| {
                format!("failed to connect to postgres via {masked_descriptor}")
            })?;
            tokio::spawn(async move {
                if let Err(error) = connection.await {
                    tracing::error!(?error, "postgres connection task ended with error");
                }
            });
            Ok(client)
        }
    }
}

fn build_tls_connector() -> Result<MakeTlsConnector> {
    let connector = NativeTlsConnector::builder()
        .build()
        .context("failed to build native TLS connector")?;
    Ok(MakeTlsConnector::new(connector))
}

pub(super) fn safe_postgres_descriptor(dsn: &str) -> String {
    dsn.parse::<PostgresConfig>()
        .map(|config| safe_postgres_descriptor_from_config(&config))
        .unwrap_or_else(|_| "postgres://[redacted-invalid-dsn]".to_string())
}

fn safe_postgres_descriptor_from_config(config: &PostgresConfig) -> String {
    let user = config.get_user().unwrap_or("unknown");
    let dbname = config.get_dbname().unwrap_or("postgres");
    let ssl_mode = match config.get_ssl_mode() {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
        _ => "unknown",
    };
    let host = config
        .get_hosts()
        .first()
        .map(postgres_host_label)
        .unwrap_or_else(|| "localhost".to_string());
    let port = config.get_ports().first().copied().unwrap_or(5432);
    format!(
        "postgres://{}:***@{}:{}/{}?sslmode={}",
        user, host, port, dbname, ssl_mode
    )
}

fn postgres_host_label(host: &Host) -> String {
    match host {
        Host::Tcp(host) => host.clone(),
        #[cfg(unix)]
        Host::Unix(path) => format!("unix:{}", path.display()),
    }
}

pub async fn bootstrap_schema(client: &Client, cfg: &AppConfig) -> Result<()> {
    let cache_key = bootstrap_schema_cache_key(cfg);
    if bootstrap_schema_cache_contains(&cache_key) && bootstrap_schema_is_current(client).await? {
        return Ok(());
    }
    client
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&BOOTSTRAP_SCHEMA_ADVISORY_LOCK_KEY],
        )
        .await
        .context("failed to acquire postgres schema bootstrap advisory lock")?;
    let schema_result: Result<()> = async {
        if !bootstrap_schema_is_current(client).await? {
            client
                .batch_execute(include_str!("../../sql/000_bootstrap.sql"))
                .await
                .context("failed to apply postgres schema")?;
        }
        ensure_app_role(client, cfg).await?;
        if bootstrap_schema_is_current(client).await? {
            bootstrap_schema_cache_insert(cache_key.clone());
        }
        Ok(())
    }
    .await;
    let unlock_result = client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&BOOTSTRAP_SCHEMA_ADVISORY_LOCK_KEY],
        )
        .await
        .context("failed to release postgres schema bootstrap advisory lock");
    match (schema_result, unlock_result) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), Ok(_)) => Err(error),
        (Ok(()), Err(unlock_error)) => Err(unlock_error),
        (Err(error), Err(unlock_error)) => Err(anyhow!(
            "{error:#}\nsecondary unlock failure: {unlock_error:#}"
        )),
    }
}

fn bootstrap_schema_cache_key(cfg: &AppConfig) -> String {
    format!("{}::{}", cfg.postgres_dsn, cfg.app_db_user)
}

pub(super) fn bootstrap_schema_cache_contains(cache_key: &str) -> bool {
    BOOTSTRAP_SCHEMA_CACHE
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .ok()
        .is_some_and(|guard| guard.contains(cache_key))
}

pub(super) fn bootstrap_schema_cache_insert(cache_key: String) {
    if let Ok(mut guard) = BOOTSTRAP_SCHEMA_CACHE
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        guard.insert(cache_key);
    }
}

async fn bootstrap_schema_is_current(client: &Client) -> Result<bool> {
    let row = client
        .query_one(
            r#"
            SELECT
                to_regclass('ami.workspaces') IS NOT NULL
                AND to_regclass('ami.teams') IS NOT NULL
                AND to_regclass('ami.transfer_policies') IS NOT NULL
                AND to_regclass('ami.import_packets') IS NOT NULL
                AND to_regclass('ami.skill_cards') IS NOT NULL
                AND to_regclass('ami.skill_evidence_bundles') IS NOT NULL
                AND to_regclass('ami.skill_trial_runs') IS NOT NULL
                AND to_regclass('ami.skill_evals') IS NOT NULL
                AND to_regclass('ami.skill_trigger_matches') IS NOT NULL
                AND to_regclass('ami.skill_reuse_logs') IS NOT NULL
                AND to_regclass('ami.projects') IS NOT NULL
                AND to_regclass('ami.project_repo_roots') IS NOT NULL
                AND to_regclass('ami.namespaces') IS NOT NULL
                AND to_regclass('ami.memory_items') IS NOT NULL
                AND to_regclass('ami.memory_envelopes') IS NOT NULL
                AND to_regclass('ami.memory_edges') IS NOT NULL
                AND to_regclass('ami.memory_conflicts') IS NOT NULL
                AND to_regclass('ami.memory_provenance') IS NOT NULL
                AND to_regclass('ami.retrieval_traces') IS NOT NULL
                AND to_regclass('ami.restore_packs') IS NOT NULL
                AND to_regclass('ami.policy_rules') IS NOT NULL
                AND to_regclass('ami.quarantine_items') IS NOT NULL
                AND to_regclass('ami.observability_snapshots') IS NOT NULL
                AND to_regclass('ami.task_nodes') IS NOT NULL
                AND to_regclass('ami.task_events') IS NOT NULL
                AND to_regclass('ami.memory_link_decisions') IS NOT NULL
                AND to_regclass('ami.pending_link_proposals') IS NOT NULL
                AND to_regclass('ami.execctl_task_leases') IS NOT NULL
                AND EXISTS (
                    SELECT 1
                    FROM pg_constraint c
                    INNER JOIN pg_class t ON t.oid = c.conrelid
                    INNER JOIN pg_namespace n ON n.oid = t.relnamespace
                    WHERE n.nspname = 'ami'
                      AND t.relname = 'skill_cards'
                      AND c.conname = 'skill_cards_candidate_class_check'
                      AND pg_get_constraintdef(c.oid) LIKE '%failure_pattern%'
                      AND pg_get_constraintdef(c.oid) LIKE '%failure_playbook%'
                      AND pg_get_constraintdef(c.oid) LIKE '%repair_sequence%'
                      AND pg_get_constraintdef(c.oid) LIKE '%anti_pattern%'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'owner_agent_id'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'trust_state'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'source_event_ids'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'message_refs'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'evidence_span'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'derivation_kind'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'ingest_seq'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'object_version'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'retention_class'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_items'
                      AND column_name = 'schema_version'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_provenance'
                      AND column_name = 'message_refs'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_provenance'
                      AND column_name = 'evidence_span'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_provenance'
                      AND column_name = 'derivation_kind'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'memory_provenance'
                      AND column_name = 'schema_version'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'projects'
                      AND column_name = 'workspace_id'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'projects'
                      AND column_name = 'visibility_scope'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'project_relations'
                      AND column_name = 'project_link_type'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'project_relations'
                      AND column_name = 'relation_status'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'agents'
                      AND column_name = 'workspace_id'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'observability_snapshots'
                      AND column_name = 'captured_at_epoch_ms'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'observability_snapshots'
                      AND column_name = 'source_event_id'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'observability_snapshots'
                      AND column_name = 'event_key'
                )
                AND EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'ami'
                      AND table_name = 'context_packs'
                      AND column_name = 'artifact_state'
                )
                AND to_regclass('ami.idx_ami_observability_snapshots_kind_event_key') IS NOT NULL
                AND to_regclass('ami.idx_ami_observability_working_state_retrieval_thread_captured') IS NOT NULL
            "#,
            &[],
        )
        .await
        .context("failed to validate postgres schema bootstrap sentinel")?;
    Ok(row.get::<_, bool>(0))
}

pub(super) fn validate_stage2_basis(
    surface_name: &str,
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
) -> Result<()> {
    let has_source_events = source_event_ids
        .as_array()
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    let has_artifact_refs = artifact_refs
        .as_array()
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    let has_message_refs = message_refs
        .as_array()
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    let has_evidence_span = evidence_span
        .as_object()
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    if derivation_kind != "operator_write"
        && !has_source_events
        && !has_artifact_refs
        && !has_message_refs
        && !has_evidence_span
    {
        return Err(anyhow!(
            "{surface_name} requires recorded basis unless derivation_kind=operator_write"
        ));
    }
    Ok(())
}

pub(super) fn conflict_state_to_edge_state(conflict_state: &str) -> &'static str {
    match conflict_state {
        "open" => "active",
        "quarantined" => "quarantined",
        "archived" => "archived",
        "resolved" | "dismissed" => "inactive",
        _ => "active",
    }
}

pub(super) fn conflict_state_to_edge_trust_state(
    conflict_state: &str,
    derivation_kind: &str,
) -> &'static str {
    match conflict_state {
        "quarantined" => "quarantined",
        _ if derivation_kind == "operator_write" => "verified",
        _ => "disputed",
    }
}

pub(super) fn sql_ident(input: &str) -> Result<String> {
    if input.is_empty()
        || !input
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(anyhow!("unsafe SQL identifier: {input}"));
    }
    Ok(input.to_string())
}

pub(super) fn sql_literal(input: &str) -> String {
    format!("'{}'", input.replace('\'', "''"))
}
