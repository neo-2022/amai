use crate::cli::VerifyWorkflowTraceArgs;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const TRACE_ARTIFACT_VERSION: &str = "agent-workflow-execution-trace-v1";
const GUARD_VERSION: &str = "agent-workflow-guard-v2";
const TRACE_SOURCE_OF_TRUTH: &str = "AGENTS.md#mandatory-specialist-team-workflow";
const EVIDENCE_MANIFEST_VERSION: &str = "workflow-evidence-manifest-v1";
const EVIDENCE_ARTIFACT_VERSION: &str = "workflow-evidence-v1";
const EVIDENCE_ROOT: &str = ".amai/continuity/workflow-evidence";

struct EvidenceItem {
    kind: String,
    content: Value,
}

pub fn run_workflow_trace_verifier(args: &VerifyWorkflowTraceArgs) -> Result<()> {
    let state = read_json_file(&args.state.display().to_string())?;
    let startup_contract = read_json_file(&args.startup_contract.display().to_string())?;
    let input = read_json_file(&args.input.display().to_string())?;
    let repo_root = args.repo_root.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize repo root {}",
            args.repo_root.display()
        )
    })?;
    let summary = validate_workflow_trace(
        &state,
        &startup_contract,
        &input,
        &repo_root,
        args.fail_on_blocking_startup_gate,
    )
    .with_context(|| {
        format!(
            "workflow trace validation failed for input {}",
            args.input.display()
        )
    })?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

pub fn validate_workflow_trace(
    state: &Value,
    startup_contract: &Value,
    input: &Value,
    repo_root: &Path,
    fail_on_blocking_startup_gate: bool,
) -> Result<Value> {
    let guard = validated_guard_snapshot(state, startup_contract)?;
    if fail_on_blocking_startup_gate {
        let blocking = bool_at(state, &["startup_execution_gate", "blocking"])?;
        if blocking {
            bail!("startup_execution_gate is blocking; report is not allowed");
        }
    }

    let trace = workflow_trace(input)?;
    validate_trace_shape(trace, guard)?;
    validate_scope(state, startup_contract, trace)?;
    validate_plan_items(input, trace, guard)?;
    validate_proof_bundle(input)?;
    validate_specialists(input, guard)?;
    validate_objections_and_final_audit(input, trace, guard)?;
    let evidence_manifest_hash = validate_evidence_manifest(input, trace, repo_root)?;

    let guard_snapshot_hash = canonical_sha256(guard)?;
    let trace_hash = canonical_sha256(trace)?;
    let declared_trace_hash = string_at(input, &["workflow_execution_trace_hash"])?;
    if declared_trace_hash != trace_hash {
        bail!(
            "workflow_execution_trace_hash does not match canonical trace hash: expected {trace_hash}, got {declared_trace_hash}"
        );
    }
    let consensus_fingerprint = consensus_fingerprint(input)?;

    Ok(json!({
        "artifact_version": "workflow-trace-validation-summary-v1",
        "status": "ok",
        "guard_snapshot_hash": guard_snapshot_hash,
        "workflow_execution_trace_hash": trace_hash,
        "evidence_manifest_hash": evidence_manifest_hash,
        "consensus_fingerprint": consensus_fingerprint,
        "stable_workline_identity": stable_workline_identity(state, startup_contract)?,
        "trace_artifact_version": TRACE_ARTIFACT_VERSION
    }))
}

pub fn consensus_fingerprint(input: &Value) -> Result<String> {
    let trace = workflow_trace(input)?;
    let trace_hash = canonical_sha256(trace)?;
    let value = json!({
        "max_age_ms": input.get("max_age_ms").and_then(Value::as_u64).unwrap_or(1_800_000),
        "plan_items": required_value(input, "plan_items")?,
        "proof_bundle": required_value(input, "proof_bundle")?,
        "evidence_manifest": required_value(input, "evidence_manifest")?,
        "required_roles": required_value(input, "required_roles")?,
        "specialists": required_value(input, "specialists")?,
        "open_objections": input.get("open_objections").cloned().unwrap_or_else(|| json!([])),
        "open_objections_count": input.get("open_objections_count").and_then(Value::as_u64).unwrap_or(0),
        "final_bughunter_pass": required_value(input, "final_bughunter_pass")?,
        "workflow_execution_trace": trace,
        "workflow_execution_trace_hash": trace_hash
    });
    canonical_sha256(&value)
}

pub fn canonical_sha256(value: &Value) -> Result<String> {
    let normalized = canonicalize(value);
    let bytes = serde_json::to_vec(&normalized)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

fn read_json_file(path: &str) -> Result<Value> {
    let raw = fs::read_to_string(path).with_context(|| format!("failed to read {path}"))?;
    serde_json::from_str(&raw).with_context(|| format!("failed to parse JSON from {path}"))
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        Value::Object(map) => {
            let mut ordered = Map::new();
            for (key, value) in map.iter().collect::<BTreeMap<_, _>>() {
                ordered.insert(key.clone(), canonicalize(value));
            }
            Value::Object(ordered)
        }
        _ => value.clone(),
    }
}

fn validated_guard_snapshot<'a>(
    state: &'a Value,
    startup_contract: &'a Value,
) -> Result<&'a Value> {
    let state_guard = value_at(state, &["agent_workflow_guard"])?;
    let contract_guard = value_at(
        startup_contract,
        &["startup_contract", "agent_workflow_guard"],
    )?;
    if state_guard != contract_guard {
        bail!("runtime agent_workflow_guard does not match pinned startup contract guard");
    }
    let expected_contract_sha = string_at(startup_contract, &["startup_contract_sha256"])?;
    let state_contract_sha = string_at(state, &["startup_contract_sha256"])?;
    if state_contract_sha != expected_contract_sha {
        bail!("runtime startup_contract_sha256 does not match startup contract");
    }
    if string_at(state_guard, &["guard_version"])? != GUARD_VERSION {
        bail!("unexpected workflow guard version");
    }
    if string_at(state_guard, &["source_of_truth"])? != TRACE_SOURCE_OF_TRUTH {
        bail!("unexpected workflow guard source_of_truth");
    }
    Ok(state_guard)
}

fn workflow_trace(input: &Value) -> Result<&Value> {
    if input
        .get("artifact_version")
        .and_then(Value::as_str)
        .is_some_and(|version| version == TRACE_ARTIFACT_VERSION)
    {
        return Ok(input);
    }
    value_at(input, &["workflow_execution_trace"])
}

fn validate_trace_shape(trace: &Value, guard: &Value) -> Result<()> {
    if string_at(trace, &["artifact_version"])? != TRACE_ARTIFACT_VERSION {
        bail!("workflow trace artifact_version mismatch");
    }
    if string_at(trace, &["guard_version"])? != GUARD_VERSION {
        bail!("workflow trace guard_version mismatch");
    }
    if string_at(trace, &["source_of_truth"])? != TRACE_SOURCE_OF_TRUTH {
        bail!("workflow trace source_of_truth mismatch");
    }
    let expected_guard_hash = canonical_sha256(guard)?;
    let actual_guard_hash = string_at(trace, &["guard_snapshot_hash"])?;
    if actual_guard_hash != expected_guard_hash {
        bail!(
            "workflow trace guard_snapshot_hash mismatch: expected {expected_guard_hash}, got {actual_guard_hash}"
        );
    }
    validate_stage_records(trace, guard)?;
    Ok(())
}

fn validate_stage_records(trace: &Value, guard: &Value) -> Result<()> {
    let expected_stages = string_array_at(guard, &["workflow_cycle", "ordered_stage_codes"])?;
    let records = array_at(trace, &["stage_records"])?;
    if records.len() != expected_stages.len() {
        bail!("workflow stage_records length mismatch");
    }
    for (idx, expected_stage) in expected_stages.iter().enumerate() {
        let record = &records[idx];
        if string_at(record, &["stage"])? != *expected_stage {
            bail!("workflow stage order mismatch at index {idx}");
        }
        let status = string_at(record, &["status"])?;
        if expected_stage == "report" {
            if status != "ready_for_report" {
                bail!("report stage must be ready_for_report before final user report");
            }
        } else if status != "completed" {
            bail!("workflow stage {expected_stage} is not completed");
        }
        require_non_empty_string(record, &["event_id"])?;
        require_non_empty_string(record, &["evidence_sha256"])?;
    }
    Ok(())
}

fn validate_scope(state: &Value, startup_contract: &Value, trace: &Value) -> Result<()> {
    let expected = stable_workline_identity(state, startup_contract)?;
    let scope = value_at(trace, &["workflow_scope"])?;
    for key in [
        "current_user_redirect_id",
        "promoted_user_redirect_id",
        "working_state_lineage_authoritative_event_id",
        "active_lease_source_event_id",
        "active_workline_headline",
        "active_workline_source_kind",
        "active_lease_source_kind",
        "startup_contract_sha256",
    ] {
        if string_at(scope, &[key])? != string_at(&expected, &[key])? {
            bail!("workflow trace scope mismatch for {key}");
        }
    }
    Ok(())
}

fn stable_workline_identity(state: &Value, startup_contract: &Value) -> Result<Value> {
    let current = string_at(
        state,
        &["workflow_promotion_state", "current_user_redirect_id"],
    )?;
    let promoted = string_at(
        state,
        &["workflow_promotion_state", "promoted_user_redirect_id"],
    )?;
    let lineage = string_at(
        state,
        &["working_state_restore_lineage", "authoritative_event_id"],
    )?;
    let lease = string_at(
        state,
        &[
            "continuity_startup_summary",
            "execctl_active_lease",
            "source_event_id",
        ],
    )?;
    if current != promoted || current != lineage || current != lease {
        bail!("stable workflow identity fields do not match");
    }
    let promotion_workline_headline = string_at(
        state,
        &["workflow_promotion_state", "active_workline_headline"],
    )?;
    let promotion_lease_headline = string_at(
        state,
        &["workflow_promotion_state", "active_lease_headline"],
    )?;
    let summary_headline = string_at(state, &["continuity_startup_summary", "headline"])?;
    let summary_lease_headline = string_at(
        state,
        &[
            "continuity_startup_summary",
            "execctl_active_lease",
            "headline",
        ],
    )?;
    let top_level_lease_headline = string_at(state, &["execctl_active_lease", "headline"])?;
    let lineage_headline = string_at(
        state,
        &["working_state_restore_lineage", "authoritative_headline"],
    )?;
    for candidate in [
        &promotion_lease_headline,
        &summary_headline,
        &summary_lease_headline,
        &top_level_lease_headline,
        &lineage_headline,
    ] {
        if candidate != &promotion_workline_headline {
            bail!("stable workflow headline fields do not match");
        }
    }
    let promotion_workline_source_kind = string_at(
        state,
        &["workflow_promotion_state", "active_workline_source_kind"],
    )?;
    let promotion_lease_source_kind = string_at(
        state,
        &["workflow_promotion_state", "active_lease_source_kind"],
    )?;
    let summary_lease_source_kind = string_at(
        state,
        &[
            "continuity_startup_summary",
            "execctl_active_lease",
            "source_kind",
        ],
    )?;
    let top_level_lease_source_kind = string_at(state, &["execctl_active_lease", "source_kind"])?;
    let lineage_source_kind = string_at(
        state,
        &["working_state_restore_lineage", "authoritative_source_kind"],
    )?;
    for candidate in [
        &promotion_lease_source_kind,
        &summary_lease_source_kind,
        &top_level_lease_source_kind,
        &lineage_source_kind,
    ] {
        if candidate != &promotion_workline_source_kind {
            bail!("stable workflow source_kind fields do not match");
        }
    }
    Ok(json!({
        "current_user_redirect_id": current,
        "promoted_user_redirect_id": promoted,
        "working_state_lineage_authoritative_event_id": lineage,
        "active_lease_source_event_id": lease,
        "active_workline_headline": promotion_workline_headline,
        "active_workline_source_kind": promotion_workline_source_kind,
        "active_lease_source_kind": promotion_lease_source_kind,
        "startup_contract_sha256": string_at(startup_contract, &["startup_contract_sha256"])?
    }))
}

fn validate_plan_items(input: &Value, trace: &Value, guard: &Value) -> Result<()> {
    let required_fields = string_array_at(guard, &["planning", "plan_item_required_fields"])?;
    let plan_items = array_at(input, &["plan_items"])?;
    if plan_items.is_empty() {
        bail!("plan_items must not be empty");
    }
    let mut ids = Vec::new();
    for item in plan_items {
        let id = string_at(item, &["id"])?;
        require_non_empty_string(item, &["status"])?;
        if string_at(item, &["status"])? != "completed" {
            bail!("plan item {id} is not completed");
        }
        for field in &required_fields {
            require_non_empty_string(item, &[field])?;
        }
        ids.push(id);
    }
    let reviews = array_at(trace, &["plan_item_reviews"])?;
    if reviews.len() != ids.len() {
        bail!("plan_item_reviews length mismatch");
    }
    let expected_roles = string_array_at(guard, &["team_critique", "roles"])?;
    let expected_critique_status = string_at(
        guard,
        &[
            "team_critique",
            "approval_status_code_before_implementation",
        ],
    )?;
    let expected_verify_status = string_at(
        guard,
        &[
            "team_verification",
            "approval_status_code_after_verification",
        ],
    )?;
    let expected_dimensions =
        string_array_at(guard, &["team_verification", "verification_dimensions"])?;
    for id in ids {
        let review = reviews
            .iter()
            .find(|review| {
                string_at(review, &["plan_item_id"])
                    .map(|value| value == id)
                    .unwrap_or(false)
            })
            .ok_or_else(|| anyhow!("missing plan item review for {id}"))?;
        validate_role_approvals(
            array_at(review, &["team_critique", "role_approvals"])?,
            &expected_roles,
            &expected_critique_status,
            "team critique",
        )?;
        if string_at(review, &["team_critique", "status"])? != expected_critique_status {
            bail!("team critique status mismatch for {id}");
        }
        if string_at(review, &["implementation", "status"])? != "completed" {
            bail!("implementation is not completed for {id}");
        }
        if !bool_at(review, &["implementation", "after_team_critique"])? {
            bail!("implementation is not after team critique for {id}");
        }
        if string_at(review, &["team_verification", "status"])? != expected_verify_status {
            bail!("team verification status mismatch for {id}");
        }
        if !bool_at(review, &["team_verification", "after_implementation"])? {
            bail!("team verification is not after implementation for {id}");
        }
        exact_string_set(
            &string_array_at(review, &["team_verification", "dimensions"])?,
            &expected_dimensions,
            "team verification dimensions",
        )?;
        validate_role_approvals(
            array_at(review, &["team_verification", "role_approvals"])?,
            &expected_roles,
            &expected_verify_status,
            "team verification",
        )?;
        if string_at(review, &["local_reverify_status"])? != "passed" {
            bail!("local reverify did not pass for {id}");
        }
        if review
            .get("unresolved_issues")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false)
        {
            bail!("unresolved issues remain for {id}");
        }
    }
    Ok(())
}

fn validate_role_approvals(
    approvals: &[Value],
    expected_roles: &[String],
    expected_status: &str,
    label: &str,
) -> Result<()> {
    if approvals.len() != expected_roles.len() {
        bail!("{label} role approval count mismatch");
    }
    let mut roles = Vec::new();
    for approval in approvals {
        let role = string_at(approval, &["role_id"])?;
        let status = string_at(approval, &["status"])?;
        if status != expected_status {
            bail!("{label} approval status mismatch for {role}");
        }
        require_non_empty_string(approval, &["agent_id"])?;
        require_non_empty_string(approval, &["evidence_sha256"])?;
        roles.push(role);
    }
    exact_string_set(&roles, expected_roles, label)
}

fn validate_proof_bundle(input: &Value) -> Result<()> {
    let commands = array_at(input, &["proof_bundle", "commands"])?;
    if commands.is_empty() {
        bail!("proof_bundle.commands must not be empty");
    }
    for command in commands {
        require_non_empty_string(command, &["command"])?;
        if string_at(command, &["status"])? != "passed" {
            bail!("proof command did not pass");
        }
        if number_at(command, &["exit_code"])? != 0 {
            bail!("proof command exit_code is not zero");
        }
        let completed_at = number_at(command, &["completed_at_epoch_ms"])?;
        if completed_at == 0 {
            bail!("proof command completed_at_epoch_ms must be positive");
        }
        require_non_empty_string(command, &["evidence_sha256"])?;
    }
    Ok(())
}

fn validate_specialists(input: &Value, guard: &Value) -> Result<()> {
    let expected_roles = string_array_at(guard, &["team_critique", "roles"])?;
    exact_string_set(
        &string_array_at(input, &["required_roles"])?,
        &expected_roles,
        "required_roles",
    )?;
    let specialists = array_at(input, &["specialists"])?;
    if specialists.len() != expected_roles.len() {
        bail!("specialist count mismatch");
    }
    let mut roles = Vec::new();
    for specialist in specialists {
        let role = string_at(specialist, &["role_id"])?;
        if string_at(specialist, &["decision"])? != "CONSENSUS_GREEN" {
            bail!("specialist decision is not green for {role}");
        }
        require_non_empty_string(specialist, &["agent_id"])?;
        require_non_empty_string(specialist, &["evidence_sha256"])?;
        roles.push(role);
    }
    exact_string_set(&roles, &expected_roles, "specialists")
}

fn validate_objections_and_final_audit(input: &Value, trace: &Value, guard: &Value) -> Result<()> {
    if input
        .get("open_objections_count")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        != 0
    {
        bail!("open_objections_count is not zero");
    }
    if input
        .get("open_objections")
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false)
    {
        bail!("open objections are present");
    }
    let final_pass = value_at(input, &["final_bughunter_pass"])?;
    if string_at(final_pass, &["status"])? != "CONSENSUS_GREEN" {
        bail!("final bughunter pass is not green");
    }
    if !bool_at(final_pass, &["report_allowed"])? {
        bail!("final bughunter pass does not allow report");
    }
    let active_workline_headline = string_at(
        value_at(trace, &["workflow_scope"])?,
        &["active_workline_headline"],
    )?;
    let final_scope = string_at(final_pass, &["scope"])?;
    if final_scope != active_workline_headline {
        bail!("final bughunter pass scope does not match active workline headline");
    }
    let final_audit = value_at(trace, &["final_audit"])?;
    if string_at(final_audit, &["status"])? != "CONSENSUS_GREEN" {
        bail!("final audit is not green");
    }
    if !bool_at(final_audit, &["report_allowed"])? {
        bail!("final audit does not allow report");
    }
    if final_audit
        .get("open_issues")
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false)
    {
        bail!("final audit has open issues");
    }
    let expected_roles = string_array_at(guard, &["team_critique", "roles"])?;
    validate_role_approvals(
        array_at(final_audit, &["role_approvals"])?,
        &expected_roles,
        "no_defects_found",
        "final audit",
    )
}

fn validate_evidence_manifest(input: &Value, trace: &Value, repo_root: &Path) -> Result<String> {
    let manifest = value_at(input, &["evidence_manifest"])?;
    if string_at(manifest, &["artifact_version"])? != EVIDENCE_MANIFEST_VERSION {
        bail!("evidence_manifest artifact_version mismatch");
    }
    let items = array_at(manifest, &["items"])?;
    if items.is_empty() {
        bail!("evidence_manifest.items must not be empty");
    }

    let mut manifest_by_hash = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for item in items {
        let id = string_at(item, &["id"])?;
        let kind = string_at(item, &["kind"])?;
        let path = string_at(item, &["path"])?;
        let sha = string_at(item, &["sha256"])?;
        validate_sha256(&sha, "evidence_manifest.items.sha256")?;
        if !ids.insert(id.clone()) {
            bail!("duplicate evidence manifest id: {id}");
        }
        if !paths.insert(path.clone()) {
            bail!("duplicate evidence manifest path: {path}");
        }
        if kind.trim().is_empty() {
            bail!("evidence manifest kind must not be empty");
        }
        let content = validate_evidence_file(repo_root, &path, &sha, &kind)?;
        if manifest_by_hash
            .insert(sha.clone(), EvidenceItem { kind, content })
            .is_some()
        {
            bail!("duplicate evidence manifest sha256: {sha}");
        }
    }

    let mut used_hashes = BTreeSet::new();
    validate_trace_evidence_bindings(trace, &manifest_by_hash, &mut used_hashes)?;
    validate_input_evidence_bindings(input, &manifest_by_hash, &mut used_hashes)?;
    if used_hashes.is_empty() {
        bail!("workflow evidence set must not be empty");
    }
    let manifest_hashes: BTreeSet<_> = manifest_by_hash.keys().cloned().collect();
    if used_hashes != manifest_hashes {
        bail!("evidence_manifest contains entries not referenced by workflow evidence");
    }

    canonical_sha256(manifest)
}

fn validate_trace_evidence_bindings(
    trace: &Value,
    manifest: &BTreeMap<String, EvidenceItem>,
    used: &mut BTreeSet<String>,
) -> Result<()> {
    for record in array_at(trace, &["stage_records"])? {
        let evidence = require_evidence(record, "workflow_stage", manifest, used, "stage record")?;
        require_string_match(record, "stage", evidence, "stage", "stage evidence")?;
        require_string_match(record, "status", evidence, "status", "stage evidence")?;
        require_string_match(record, "event_id", evidence, "event_id", "stage evidence")?;
    }
    for review in array_at(trace, &["plan_item_reviews"])? {
        for approval in array_at(review, &["team_critique", "role_approvals"])? {
            validate_role_evidence(approval, manifest, used, "team critique role evidence")?;
        }
        for approval in array_at(review, &["team_verification", "role_approvals"])? {
            validate_role_evidence(approval, manifest, used, "team verification role evidence")?;
        }
    }
    for approval in array_at(trace, &["final_audit", "role_approvals"])? {
        validate_role_evidence(approval, manifest, used, "final audit role evidence")?;
    }
    Ok(())
}

fn validate_input_evidence_bindings(
    input: &Value,
    manifest: &BTreeMap<String, EvidenceItem>,
    used: &mut BTreeSet<String>,
) -> Result<()> {
    for command in array_at(input, &["proof_bundle", "commands"])? {
        let evidence = require_evidence(command, "proof_command", manifest, used, "proof command")?;
        require_string_match(
            command,
            "command",
            evidence,
            "command",
            "proof command evidence",
        )?;
        require_string_match(
            command,
            "status",
            evidence,
            "status",
            "proof command evidence",
        )?;
        require_u64_match(
            command,
            "exit_code",
            evidence,
            "exit_code",
            "proof command evidence",
        )?;
        require_u64_match(
            command,
            "completed_at_epoch_ms",
            evidence,
            "completed_at_epoch_ms",
            "proof command evidence",
        )?;
    }
    for specialist in array_at(input, &["specialists"])? {
        let evidence =
            require_evidence(specialist, "specialist_role", manifest, used, "specialist")?;
        require_string_match(
            specialist,
            "role_id",
            evidence,
            "role_id",
            "specialist evidence",
        )?;
        require_string_match(
            specialist,
            "agent_id",
            evidence,
            "agent_id",
            "specialist evidence",
        )?;
        require_string_match(
            specialist,
            "decision",
            evidence,
            "decision",
            "specialist evidence",
        )?;
    }
    let final_pass = value_at(input, &["final_bughunter_pass"])?;
    let evidence = require_evidence(
        final_pass,
        "final_bughunter_pass",
        manifest,
        used,
        "final bughunter pass",
    )?;
    require_string_match(
        final_pass,
        "status",
        evidence,
        "status",
        "final bughunter pass evidence",
    )?;
    require_bool_match(
        final_pass,
        "report_allowed",
        evidence,
        "report_allowed",
        "final bughunter pass evidence",
    )?;
    require_string_match(
        final_pass,
        "scope",
        evidence,
        "scope",
        "final bughunter pass evidence",
    )?;
    Ok(())
}

fn validate_role_evidence(
    approval: &Value,
    manifest: &BTreeMap<String, EvidenceItem>,
    used: &mut BTreeSet<String>,
    label: &str,
) -> Result<()> {
    let evidence = require_evidence(approval, "specialist_role", manifest, used, label)?;
    require_string_match(approval, "role_id", evidence, "role_id", label)?;
    require_string_match(approval, "agent_id", evidence, "agent_id", label)
}

fn require_evidence<'a>(
    value: &Value,
    expected_kind: &str,
    manifest: &'a BTreeMap<String, EvidenceItem>,
    used: &mut BTreeSet<String>,
    label: &str,
) -> Result<&'a Value> {
    let sha = string_at(value, &["evidence_sha256"])?;
    validate_sha256(&sha, "evidence_sha256")?;
    let item = manifest.get(&sha).ok_or_else(|| {
        anyhow!("{label} evidence_sha256 is not backed by evidence_manifest: {sha}")
    })?;
    if item.kind != expected_kind {
        bail!(
            "{label} evidence kind mismatch for {sha}: expected {expected_kind}, got {}",
            item.kind
        );
    }
    used.insert(sha);
    Ok(&item.content)
}

fn require_string_match(
    left: &Value,
    left_field: &str,
    right: &Value,
    right_field: &str,
    label: &str,
) -> Result<()> {
    let expected = string_at(left, &[left_field])?;
    let actual = string_at(right, &[right_field])?;
    if actual != expected {
        bail!(
            "{label} string mismatch for {left_field}/{right_field}: expected {expected}, got {actual}"
        );
    }
    Ok(())
}

fn require_u64_match(
    left: &Value,
    left_field: &str,
    right: &Value,
    right_field: &str,
    label: &str,
) -> Result<()> {
    let expected = number_at(left, &[left_field])?;
    let actual = number_at(right, &[right_field])?;
    if actual != expected {
        bail!(
            "{label} number mismatch for {left_field}/{right_field}: expected {expected}, got {actual}"
        );
    }
    Ok(())
}

fn require_bool_match(
    left: &Value,
    left_field: &str,
    right: &Value,
    right_field: &str,
    label: &str,
) -> Result<()> {
    let expected = bool_at(left, &[left_field])?;
    let actual = bool_at(right, &[right_field])?;
    if actual != expected {
        bail!(
            "{label} boolean mismatch for {left_field}/{right_field}: expected {expected}, got {actual}"
        );
    }
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{label} must be a 64-character hex SHA256 value");
    }
    let first = value.as_bytes()[0];
    if value.as_bytes().iter().all(|byte| *byte == first) {
        bail!("{label} looks like a placeholder hash");
    }
    Ok(())
}

fn validate_evidence_file(
    repo_root: &Path,
    relative_path: &str,
    expected_sha: &str,
    expected_kind: &str,
) -> Result<Value> {
    let relative = Path::new(relative_path);
    if relative.is_absolute() {
        bail!("evidence path must be workspace-relative: {relative_path}");
    }
    if !relative.starts_with(EVIDENCE_ROOT) {
        bail!("evidence path must stay under {EVIDENCE_ROOT}: {relative_path}");
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("evidence path contains unsafe component: {relative_path}"),
        }
    }

    let evidence_root = repo_root.join(EVIDENCE_ROOT);
    let evidence_root_canon = evidence_root.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize evidence root {}",
            evidence_root.display()
        )
    })?;
    let full_path = repo_root.join(relative);
    let full_canon = full_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize evidence file {relative_path}"))?;
    if !full_canon.starts_with(&evidence_root_canon) {
        bail!("evidence path escaped evidence root: {relative_path}");
    }
    assert_no_symlink_components(repo_root, relative)?;

    let metadata = fs::metadata(&full_path)
        .with_context(|| format!("failed to stat evidence file {relative_path}"))?;
    if !metadata.is_file() {
        bail!("evidence path is not a regular file: {relative_path}");
    }
    let bytes = fs::read(&full_path)
        .with_context(|| format!("failed to read evidence file {relative_path}"))?;
    let actual_sha = sha256_bytes(&bytes);
    if actual_sha != expected_sha {
        bail!(
            "evidence file hash mismatch for {relative_path}: expected {expected_sha}, got {actual_sha}"
        );
    }
    let content: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse evidence JSON from {relative_path}"))?;
    if string_at(&content, &["artifact_version"])? != EVIDENCE_ARTIFACT_VERSION {
        bail!("evidence artifact_version mismatch for {relative_path}");
    }
    let actual_kind = string_at(&content, &["kind"])?;
    if actual_kind != expected_kind {
        bail!(
            "evidence file kind mismatch for {relative_path}: expected {expected_kind}, got {actual_kind}"
        );
    }
    Ok(content)
}

fn assert_no_symlink_components(repo_root: &Path, relative: &Path) -> Result<()> {
    let mut cursor = PathBuf::from(repo_root);
    for component in relative.components() {
        let Component::Normal(part) = component else {
            bail!("evidence path contains unsafe component");
        };
        cursor.push(part);
        let metadata = fs::symlink_metadata(&cursor)
            .with_context(|| format!("failed to inspect evidence path {}", cursor.display()))?;
        if metadata.file_type().is_symlink() {
            bail!(
                "symlink is not allowed in evidence path {}",
                cursor.display()
            );
        }
    }
    Ok(())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn exact_string_set(actual: &[String], expected: &[String], label: &str) -> Result<()> {
    let actual_set: BTreeSet<_> = actual.iter().cloned().collect();
    let expected_set: BTreeSet<_> = expected.iter().cloned().collect();
    if actual_set != expected_set || actual.len() != expected.len() {
        bail!("{label} set mismatch");
    }
    Ok(())
}

fn required_value(input: &Value, key: &str) -> Result<Value> {
    input
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow!("missing required field {key}"))
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    let mut current = value;
    for key in path {
        current = current
            .get(*key)
            .ok_or_else(|| anyhow!("missing JSON field {}", path.join(".")))?;
    }
    Ok(current)
}

fn string_at(value: &Value, path: &[&str]) -> Result<String> {
    value_at(value, path)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("{} must be a non-empty string", path.join(".")))
}

fn require_non_empty_string(value: &Value, path: &[&str]) -> Result<()> {
    string_at(value, path).map(|_| ())
}

fn bool_at(value: &Value, path: &[&str]) -> Result<bool> {
    value_at(value, path)?
        .as_bool()
        .ok_or_else(|| anyhow!("{} must be a boolean", path.join(".")))
}

fn number_at(value: &Value, path: &[&str]) -> Result<u64> {
    value_at(value, path)?
        .as_u64()
        .ok_or_else(|| anyhow!("{} must be an unsigned integer", path.join(".")))
}

fn array_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a [Value]> {
    value_at(value, path)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow!("{} must be an array", path.join(".")))
}

fn string_array_at(value: &Value, path: &[&str]) -> Result<Vec<String>> {
    let items = array_at(value, path)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(
            item.as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("{} must contain only non-empty strings", path.join(".")))?
                .to_string(),
        );
    }
    Ok(out)
}
