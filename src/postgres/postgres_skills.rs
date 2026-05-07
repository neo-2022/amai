use super::*;

fn value_string_array_matches(value: &Value, candidate: Option<&str>) -> bool {
    let Some(candidate) = candidate else {
        return true;
    };
    let Some(items) = value.as_array() else {
        return true;
    };
    if items.is_empty() {
        return true;
    }
    items.iter().any(|item| item.as_str() == Some(candidate))
}

fn value_string_array_matches_required(value: &Value, candidate: Option<&str>) -> bool {
    let Some(items) = value.as_array() else {
        return true;
    };
    if items.is_empty() {
        return true;
    }
    let Some(candidate) = candidate else {
        return false;
    };
    items.iter().any(|item| item.as_str() == Some(candidate))
}

fn evidence_span_string_values(evidence_span: &Value, keys: &[&str]) -> Vec<String> {
    let Some(map) = evidence_span.as_object() else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        if let Some(text) = value.as_str() {
            values.push(text.to_string());
            continue;
        }
        if let Some(items) = value.as_array() {
            values.extend(
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|text| text.to_string())),
            );
        }
    }
    values
}

fn evidence_span_contexts(evidence_span: &Value) -> Vec<String> {
    let Some(map) = evidence_span.as_object() else {
        return Vec::new();
    };
    if let Some(context) = map.get("context").and_then(Value::as_str) {
        return vec![context.to_string()];
    }
    if let Some(contexts) = map.get("contexts").and_then(Value::as_array) {
        return contexts
            .iter()
            .filter_map(|value| value.as_str().map(|item| item.to_string()))
            .collect();
    }
    Vec::new()
}

async fn enforce_skill_context_constraints(
    client: &Client,
    skill_card_id: Uuid,
    evidence_span: &Value,
    activity: &str,
) -> Result<()> {
    let row = client
        .query_one(
            "SELECT skill_context_constraints FROM ami.skill_cards WHERE skill_card_id = $1",
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load skill context constraints for {skill_card_id}"))?;
    let constraints: Value = row.get(0);
    let constraints_list = constraints.as_array().cloned().unwrap_or_default();
    if constraints_list.is_empty() {
        return Ok(());
    }
    let contexts = evidence_span_contexts(evidence_span);
    if contexts.is_empty() {
        return Err(anyhow!(
            "{} requires context to match skill_context_constraints for {}",
            activity,
            skill_card_id
        ));
    }
    let matches = contexts.iter().any(|context| {
        constraints_list
            .iter()
            .any(|constraint| constraint.as_str() == Some(context.as_str()))
    });
    if !matches {
        return Err(anyhow!(
            "{} context {:?} not allowed by skill_context_constraints for {}",
            activity,
            contexts,
            skill_card_id
        ));
    }
    Ok(())
}

async fn enforce_skill_runtime_model_tool_constraints(
    client: &Client,
    skill_card_id: Uuid,
    evidence_span: &Value,
    runtime: Option<&str>,
    model: Option<&str>,
    tool: Option<&str>,
    activity: &str,
) -> Result<()> {
    let row = client
        .query_one(
            r#"
            SELECT
                skill_runtime_constraints,
                skill_model_constraints,
                skill_tool_constraints
            FROM ami.skill_cards
            WHERE skill_card_id = $1
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| {
            format!("failed to load skill runtime/model/tool constraints for {skill_card_id}")
        })?;
    let runtime_constraints: Value = row.get(0);
    let model_constraints: Value = row.get(1);
    let tool_constraints: Value = row.get(2);

    let runtime_candidates = runtime
        .map(|value| vec![value.to_string()])
        .unwrap_or_else(|| {
            evidence_span_string_values(evidence_span, &["runtime", "runtime_name", "runtimes"])
        });
    let model_candidates = model
        .map(|value| vec![value.to_string()])
        .unwrap_or_else(|| {
            evidence_span_string_values(evidence_span, &["model", "model_name", "models"])
        });
    let tool_candidates = tool
        .map(|value| vec![value.to_string()])
        .unwrap_or_else(|| {
            evidence_span_string_values(evidence_span, &["tool", "tool_name", "tools"])
        });

    enforce_skill_env_constraint(
        activity,
        "runtime",
        &runtime_constraints,
        &runtime_candidates,
        skill_card_id,
    )?;
    enforce_skill_env_constraint(
        activity,
        "model",
        &model_constraints,
        &model_candidates,
        skill_card_id,
    )?;
    enforce_skill_env_constraint(
        activity,
        "tool",
        &tool_constraints,
        &tool_candidates,
        skill_card_id,
    )?;
    Ok(())
}

fn enforce_skill_env_constraint(
    activity: &str,
    label: &str,
    constraints: &Value,
    candidates: &[String],
    skill_card_id: Uuid,
) -> Result<()> {
    let constraints_list = constraints.as_array().cloned().unwrap_or_default();
    if constraints_list.is_empty() {
        return Ok(());
    }
    if candidates.is_empty() {
        return Err(anyhow!(
            "{} requires {} to match skill_{}_constraints for {}",
            activity,
            label,
            label,
            skill_card_id
        ));
    }
    let matches = candidates.iter().any(|candidate| {
        constraints_list
            .iter()
            .any(|constraint| constraint.as_str() == Some(candidate.as_str()))
    });
    if !matches {
        return Err(anyhow!(
            "{} {} {:?} not allowed by skill_{}_constraints for {}",
            activity,
            label,
            candidates,
            label,
            skill_card_id
        ));
    }
    Ok(())
}

fn evidence_span_bool(evidence_span: &Value, key: &str) -> bool {
    evidence_span
        .as_object()
        .and_then(|map| map.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn maybe_rfc3339_utc(row: &Row, idx: usize) -> Option<String> {
    row.get::<_, Option<String>>(idx)
}

fn skill_card_record_from_row(row: &Row) -> SkillCardRecord {
    SkillCardRecord {
        skill_card_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        skill_id: row.get(4),
        skill_version: row.get(5),
        skill_title: row.get(6),
        skill_goal: row.get(7),
        skill_trigger_conditions: row.get(8),
        skill_preconditions: row.get(9),
        skill_execution_steps: row.get(10),
        skill_stop_conditions: row.get(11),
        skill_forbidden_when: row.get(12),
        skill_expected_outcome: row.get(13),
        skill_scope_type: row.get(14),
        skill_owner_scope: row.get(15),
        skill_trust_state: row.get(16),
        skill_verification_state: row.get(17),
        skill_runtime_constraints: row.get(18),
        skill_model_constraints: row.get(19),
        skill_tool_constraints: row.get(20),
        skill_context_constraints: row.get(21),
        skill_source_event_ids: row.get(22),
        skill_artifact_refs: row.get(23),
        skill_evidence_span: row.get(24),
        skill_candidate_class: row.get(25),
        skill_derivation_kind: row.get(26),
        skill_source_kind: row.get(27),
        skill_hot_path_write_eligible: row.get(28),
        skill_background_consolidation_recommended: row.get(29),
        skill_success_count: row.get(30),
        skill_failure_count: row.get(31),
        skill_reuse_count: row.get(32),
        skill_shadow_pass_count: row.get(33),
        skill_shadow_fail_count: row.get(34),
        skill_last_used_at: maybe_rfc3339_utc(row, 35),
        skill_last_verified_at: maybe_rfc3339_utc(row, 36),
        skill_patch_parent_id: row.get(37),
        skill_merge_group_id: row.get(38),
        skill_shared_promotion_state: row.get(39),
        skill_shared_approved_by: row.get(40),
        skill_shared_approval_reason: row.get(41),
        skill_shared_approved_at: maybe_rfc3339_utc(row, 42),
        skill_utility_score: row.get(43),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillRefinementAction {
    Patch,
    Merge,
    New,
}

impl SkillRefinementAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Merge => "merge",
            Self::New => "new",
        }
    }
}

#[derive(Debug, Clone)]
struct SimilarSkillCard {
    skill_card_id: Uuid,
    skill_id: String,
    skill_version: i32,
    skill_title: String,
    skill_goal: String,
    skill_candidate_class: String,
    skill_scope_type: String,
    skill_owner_scope: String,
    skill_runtime_constraints: Value,
    skill_model_constraints: Value,
    skill_tool_constraints: Value,
    skill_context_constraints: Value,
    skill_trigger_conditions: Value,
    skill_execution_steps: Value,
    skill_trust_state: String,
    skill_verification_state: String,
    skill_merge_group_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillRefinementDecision {
    action: String,
    similar_skill_card_ids: Vec<Uuid>,
    patch_parent_skill_card_id: Option<Uuid>,
    merge_group_id: Option<Uuid>,
    similarity_required_decision: bool,
}

fn normalized_skill_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalized_skill_string_set(values: &[String]) -> Vec<String> {
    let mut normalized = values
        .iter()
        .map(|value| normalized_skill_text(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalized_value_string_set(value: &Value) -> Vec<String> {
    normalized_skill_string_set(
        &value
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>(),
    )
}

fn string_sets_overlap(left: &[String], right: &[String]) -> bool {
    left.iter()
        .any(|item| right.iter().any(|other| other == item))
}

fn skills_are_similar_enough(
    existing: &SimilarSkillCard,
    skill_title: &str,
    skill_goal: &str,
    skill_candidate_class: &str,
    skill_scope_type: &str,
    skill_owner_scope: &str,
    skill_runtime_constraints: &[String],
    skill_model_constraints: &[String],
    skill_tool_constraints: &[String],
    skill_context_constraints: &[String],
    skill_trigger_conditions: &[String],
    skill_execution_steps: &[String],
) -> bool {
    if matches!(
        existing.skill_trust_state.as_str(),
        "deprecated" | "quarantined"
    ) || matches!(
        existing.skill_verification_state.as_str(),
        "deprecated" | "quarantined"
    ) {
        return false;
    }
    if existing.skill_candidate_class != skill_candidate_class
        || existing.skill_scope_type != skill_scope_type
        || existing.skill_owner_scope != skill_owner_scope
    {
        return false;
    }
    if normalized_value_string_set(&existing.skill_runtime_constraints)
        != normalized_skill_string_set(skill_runtime_constraints)
        || normalized_value_string_set(&existing.skill_model_constraints)
            != normalized_skill_string_set(skill_model_constraints)
        || normalized_value_string_set(&existing.skill_tool_constraints)
            != normalized_skill_string_set(skill_tool_constraints)
        || normalized_value_string_set(&existing.skill_context_constraints)
            != normalized_skill_string_set(skill_context_constraints)
    {
        return false;
    }

    let normalized_title = normalized_skill_text(skill_title);
    let normalized_goal = normalized_skill_text(skill_goal);
    let existing_title = normalized_skill_text(&existing.skill_title);
    let existing_goal = normalized_skill_text(&existing.skill_goal);
    let title_match = !normalized_title.is_empty() && normalized_title == existing_title;
    let goal_match = !normalized_goal.is_empty() && normalized_goal == existing_goal;

    let trigger_overlap = string_sets_overlap(
        &normalized_value_string_set(&existing.skill_trigger_conditions),
        &normalized_skill_string_set(skill_trigger_conditions),
    );
    let step_overlap = string_sets_overlap(
        &normalized_value_string_set(&existing.skill_execution_steps),
        &normalized_skill_string_set(skill_execution_steps),
    );

    (goal_match && (title_match || trigger_overlap || step_overlap))
        || (title_match && trigger_overlap && step_overlap)
}

async fn list_similar_skill_cards(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    skill_id: &str,
    skill_title: &str,
    skill_goal: &str,
    skill_candidate_class: &str,
    skill_scope_type: &str,
    skill_owner_scope: &str,
    skill_runtime_constraints: &[String],
    skill_model_constraints: &[String],
    skill_tool_constraints: &[String],
    skill_context_constraints: &[String],
    skill_trigger_conditions: &[String],
    skill_execution_steps: &[String],
) -> Result<Vec<SimilarSkillCard>> {
    let rows = client
        .query(
            r#"
            SELECT
                skill_card_id,
                skill_id,
                skill_version,
                skill_title,
                skill_goal,
                skill_candidate_class,
                skill_scope_type,
                skill_owner_scope,
                skill_runtime_constraints,
                skill_model_constraints,
                skill_tool_constraints,
                skill_context_constraints,
                skill_trigger_conditions,
                skill_execution_steps,
                skill_trust_state,
                skill_verification_state,
                skill_merge_group_id
            FROM ami.skill_cards
            WHERE project_id = $1
              AND namespace_id = $2
              AND skill_id <> $3
            "#,
            &[&project_id, &namespace_id, &skill_id],
        )
        .await
        .context("failed to list similar skill candidates")?;
    Ok(rows
        .into_iter()
        .map(|row| SimilarSkillCard {
            skill_card_id: row.get(0),
            skill_id: row.get(1),
            skill_version: row.get(2),
            skill_title: row.get(3),
            skill_goal: row.get(4),
            skill_candidate_class: row.get(5),
            skill_scope_type: row.get(6),
            skill_owner_scope: row.get(7),
            skill_runtime_constraints: row.get(8),
            skill_model_constraints: row.get(9),
            skill_tool_constraints: row.get(10),
            skill_context_constraints: row.get(11),
            skill_trigger_conditions: row.get(12),
            skill_execution_steps: row.get(13),
            skill_trust_state: row.get(14),
            skill_verification_state: row.get(15),
            skill_merge_group_id: row.get(16),
        })
        .filter(|existing| {
            skills_are_similar_enough(
                existing,
                skill_title,
                skill_goal,
                skill_candidate_class,
                skill_scope_type,
                skill_owner_scope,
                skill_runtime_constraints,
                skill_model_constraints,
                skill_tool_constraints,
                skill_context_constraints,
                skill_trigger_conditions,
                skill_execution_steps,
            )
        })
        .collect())
}

async fn get_skill_patch_parent_context(
    client: &Client,
    skill_card_id: Uuid,
) -> Result<SimilarSkillCard> {
    let row = client
        .query_opt(
            r#"
            SELECT
                skill_card_id,
                skill_id,
                skill_version,
                skill_title,
                skill_goal,
                skill_candidate_class,
                skill_scope_type,
                skill_owner_scope,
                skill_runtime_constraints,
                skill_model_constraints,
                skill_tool_constraints,
                skill_context_constraints,
                skill_trigger_conditions,
                skill_execution_steps,
                skill_trust_state,
                skill_verification_state,
                skill_merge_group_id
            FROM ami.skill_cards
            WHERE skill_card_id = $1
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load patch parent skill card {skill_card_id}"))?;
    let Some(row) = row else {
        return Err(anyhow!("patch parent skill card {skill_card_id} not found"));
    };
    Ok(SimilarSkillCard {
        skill_card_id: row.get(0),
        skill_id: row.get(1),
        skill_version: row.get(2),
        skill_title: row.get(3),
        skill_goal: row.get(4),
        skill_candidate_class: row.get(5),
        skill_scope_type: row.get(6),
        skill_owner_scope: row.get(7),
        skill_runtime_constraints: row.get(8),
        skill_model_constraints: row.get(9),
        skill_tool_constraints: row.get(10),
        skill_context_constraints: row.get(11),
        skill_trigger_conditions: row.get(12),
        skill_execution_steps: row.get(13),
        skill_trust_state: row.get(14),
        skill_verification_state: row.get(15),
        skill_merge_group_id: row.get(16),
    })
}

fn parse_skill_refinement_action(
    refinement_action: Option<&str>,
    patch_parent_skill_card_id: Option<Uuid>,
    merge_group_id: Option<Uuid>,
) -> Result<Option<SkillRefinementAction>> {
    if let Some(action) = refinement_action
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return match action {
            "patch" => Ok(Some(SkillRefinementAction::Patch)),
            "merge" => Ok(Some(SkillRefinementAction::Merge)),
            "new" => Ok(Some(SkillRefinementAction::New)),
            other => Err(anyhow!(
                "unsupported skill refinement action: {other}; expected patch|merge|new"
            )),
        };
    }
    if patch_parent_skill_card_id.is_some() {
        return Ok(Some(SkillRefinementAction::Patch));
    }
    if merge_group_id.is_some() {
        return Ok(Some(SkillRefinementAction::Merge));
    }
    Ok(None)
}

async fn decide_skill_refinement(
    client: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    skill_id: &str,
    skill_version: i32,
    skill_title: &str,
    skill_goal: &str,
    skill_candidate_class: &str,
    skill_scope_type: &str,
    skill_owner_scope: &str,
    skill_runtime_constraints: &[String],
    skill_model_constraints: &[String],
    skill_tool_constraints: &[String],
    skill_context_constraints: &[String],
    skill_trigger_conditions: &[String],
    skill_execution_steps: &[String],
    refinement_action: Option<&str>,
    patch_parent_skill_card_id: Option<Uuid>,
    merge_group_id: Option<Uuid>,
) -> Result<SkillRefinementDecision> {
    let action = parse_skill_refinement_action(
        refinement_action,
        patch_parent_skill_card_id,
        merge_group_id,
    )?;
    let similar_cards = list_similar_skill_cards(
        client,
        project.project_id,
        namespace.namespace_id,
        skill_id,
        skill_title,
        skill_goal,
        skill_candidate_class,
        skill_scope_type,
        skill_owner_scope,
        skill_runtime_constraints,
        skill_model_constraints,
        skill_tool_constraints,
        skill_context_constraints,
        skill_trigger_conditions,
        skill_execution_steps,
    )
    .await?;
    let similar_skill_card_ids = similar_cards
        .iter()
        .map(|card| card.skill_card_id)
        .collect::<Vec<_>>();

    let Some(action) = action else {
        if similar_skill_card_ids.is_empty() {
            return Ok(SkillRefinementDecision {
                action: SkillRefinementAction::New.as_str().to_string(),
                similar_skill_card_ids,
                patch_parent_skill_card_id: None,
                merge_group_id: None,
                similarity_required_decision: false,
            });
        }
        return Err(anyhow!(
            "similar skill already exists in this namespace; specify --refinement-action patch|merge|new to avoid clone explosion"
        ));
    };

    match action {
        SkillRefinementAction::Patch => {
            let parent_id = patch_parent_skill_card_id.ok_or_else(|| {
                anyhow!("skill refinement action patch requires --patch-parent-skill-card-id")
            })?;
            let parent = get_skill_patch_parent_context(client, parent_id).await?;
            if !similar_skill_card_ids.contains(&parent_id) && parent.skill_id != skill_id {
                return Err(anyhow!(
                    "patch parent {parent_id} is not a similar existing skill in this namespace"
                ));
            }
            if parent.skill_id != skill_id {
                return Err(anyhow!(
                    "patch refinement requires same skill_id as parent; got parent skill_id={} and new skill_id={}",
                    parent.skill_id,
                    skill_id
                ));
            }
            if skill_version <= parent.skill_version {
                return Err(anyhow!(
                    "patch refinement requires skill_version greater than parent version {}",
                    parent.skill_version
                ));
            }
            Ok(SkillRefinementDecision {
                action: action.as_str().to_string(),
                similar_skill_card_ids,
                patch_parent_skill_card_id: Some(parent_id),
                merge_group_id: Some(parent.skill_merge_group_id.unwrap_or(parent_id)),
                similarity_required_decision: true,
            })
        }
        SkillRefinementAction::Merge => {
            if similar_skill_card_ids.is_empty() && merge_group_id.is_none() {
                return Err(anyhow!(
                    "merge refinement requires a similar existing skill or explicit --merge-group-id"
                ));
            }
            let resolved_merge_group_id = merge_group_id.or_else(|| {
                similar_cards
                    .first()
                    .map(|card| card.skill_merge_group_id.unwrap_or(card.skill_card_id))
            });
            Ok(SkillRefinementDecision {
                action: action.as_str().to_string(),
                similar_skill_card_ids,
                patch_parent_skill_card_id: None,
                merge_group_id: resolved_merge_group_id,
                similarity_required_decision: true,
            })
        }
        SkillRefinementAction::New => Ok(SkillRefinementDecision {
            action: action.as_str().to_string(),
            similar_skill_card_ids: similar_skill_card_ids.clone(),
            patch_parent_skill_card_id: None,
            merge_group_id,
            similarity_required_decision: !similar_skill_card_ids.is_empty(),
        }),
    }
}

#[derive(Debug, Clone)]
pub(super) struct SkillCardCandidateExtraction {
    pub(super) source_basis_status: String,
    #[allow(dead_code)]
    pub(super) source_event_count: usize,
    #[allow(dead_code)]
    pub(super) artifact_ref_count: usize,
    #[allow(dead_code)]
    pub(super) has_evidence_span: bool,
    pub(super) candidate_class: String,
    pub(super) derivation_kind: String,
    pub(super) source_kind: Option<String>,
    pub(super) hot_path_write_eligible: bool,
    pub(super) background_consolidation_recommended: bool,
}

#[derive(Debug, Clone)]
pub(super) struct SkillCardPolicyScopeFilter {
    pub(super) visibility_scope: String,
    pub(super) skill_owner_scope: String,
    pub(super) project_code: String,
    pub(super) namespace_code: String,
    pub(super) owner_agent_required: bool,
    pub(super) owner_agent_present: bool,
    pub(super) private_contour_violation: bool,
    pub(super) scope_allowed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct SkillCardVerificationConflictCheck {
    pub(super) evidence_present: bool,
    pub(super) duplicate_version_conflict: bool,
    pub(super) poisoned_detected: bool,
    pub(super) private_contour_violation: bool,
    pub(super) skill_id: String,
    pub(super) skill_version: i32,
    pub(super) write_allowed: bool,
}

fn is_candidate_class(value: &str) -> bool {
    matches!(
        value,
        "fact"
            | "decision"
            | "commitment"
            | "skill_hint"
            | "artifact_ref"
            | "failure_pattern"
            | "failure_playbook"
            | "repair_sequence"
            | "anti_pattern"
    )
}

pub(super) fn canonical_candidate_class_from_hints(
    explicit: Option<&str>,
    item_kind: Option<&str>,
    title: Option<&str>,
    tags: &[String],
    has_fact_tuple: bool,
    default_class: &str,
) -> String {
    if let Some(explicit) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        if is_candidate_class(explicit) {
            return explicit.to_string();
        }
    }
    if let Some(item_kind) = item_kind {
        match item_kind {
            "decision" => return "decision".to_string(),
            "task" | "commitment" => return "commitment".to_string(),
            "skill" | "skill_hint" => return "skill_hint".to_string(),
            "artifact" | "artifact_ref" => return "artifact_ref".to_string(),
            "failure_pattern" => return "failure_pattern".to_string(),
            "failure_playbook" => return "failure_playbook".to_string(),
            "repair_sequence" => return "repair_sequence".to_string(),
            "anti_pattern" => return "anti_pattern".to_string(),
            _ => {}
        }
    }
    let normalized_tags = tags
        .iter()
        .map(|tag| tag.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let title_lower = title.unwrap_or_default().trim().to_ascii_lowercase();
    if normalized_tags.iter().any(|tag| {
        matches!(
            tag.as_str(),
            "anti_pattern" | "anti-pattern" | "antipattern"
        )
    }) {
        return "anti_pattern".to_string();
    }
    if normalized_tags.iter().any(|tag| tag == "failure_pattern") {
        return "failure_pattern".to_string();
    }
    if normalized_tags.iter().any(|tag| tag == "failure_playbook") {
        return "failure_playbook".to_string();
    }
    if normalized_tags.iter().any(|tag| tag == "repair_sequence") {
        return "repair_sequence".to_string();
    }
    if normalized_tags.iter().any(|tag| tag == "decision") || title_lower.contains("decision") {
        return "decision".to_string();
    }
    if normalized_tags.iter().any(|tag| tag == "commitment") || title_lower.contains("commitment") {
        return "commitment".to_string();
    }
    if normalized_tags.iter().any(|tag| tag == "skill_hint") || title_lower.contains("skill") {
        return "skill_hint".to_string();
    }
    if normalized_tags.iter().any(|tag| tag == "artifact_ref") || title_lower.contains("artifact") {
        return "artifact_ref".to_string();
    }
    if has_fact_tuple {
        return "fact".to_string();
    }
    default_class.to_string()
}

pub(super) fn runtime_contract_for_candidate_class(
    candidate_class: &str,
    derivation_kind: &str,
) -> (bool, bool) {
    let hot_path =
        derivation_kind == "operator_write" || matches!(candidate_class, "decision" | "commitment");
    let background = !hot_path;
    (hot_path, background)
}

#[derive(Debug, Clone)]
pub(super) struct RestorePackPolicyScopeFilter {
    workspace_code: String,
    project_code: String,
    namespace_code: String,
    pack_kind: String,
    source_snapshot_required: bool,
    source_snapshot_present: bool,
    source_snapshot_found: bool,
    source_snapshot_kind: Option<String>,
    source_snapshot_project_code: Option<String>,
    source_snapshot_namespace_code: Option<String>,
    snapshot_kind_valid: bool,
    snapshot_scope_matches: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone)]
pub(super) struct RestorePackVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    write_allowed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct RetrievalTracePolicyScopeFilter {
    workspace_code: String,
    project_code: String,
    namespace_code: String,
    context_pack_present: bool,
    context_pack_found: bool,
    context_pack_project_code: Option<String>,
    context_pack_namespace_code: Option<String>,
    context_pack_scope_matches: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone)]
pub(super) struct RetrievalTraceVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    sufficiency_basis_present: bool,
    decision_trace_consistent: bool,
    write_allowed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct MemoryProvenancePolicyScopeFilter {
    workspace_code: String,
    project_code: String,
    namespace_code: String,
    memory_item_present: bool,
    memory_item_found: bool,
    memory_item_scope_matches: bool,
    artifact_ref_present: bool,
    artifact_ref_found: bool,
    artifact_ref_scope_matches: bool,
    source_snapshot_present: bool,
    source_snapshot_found: bool,
    source_snapshot_scope_matches: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone)]
pub(super) struct MemoryProvenanceVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    write_allowed: bool,
}

fn restore_pack_marks_poisoned(evidence_span: &Value) -> bool {
    evidence_span
        .get("poisoned")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || evidence_span
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn retrieval_trace_marks_poisoned(evidence_span: &Value) -> bool {
    evidence_span
        .get("poisoned")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || evidence_span
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn memory_provenance_marks_poisoned(evidence_span: &Value) -> bool {
    evidence_span
        .get("poisoned")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || evidence_span
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

pub(super) async fn run_memory_provenance_policy_scope_filter(
    client: &Client,
    workspace_code: &str,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &MemoryProvenanceInsert<'_>,
) -> Result<MemoryProvenancePolicyScopeFilter> {
    let memory_item = match record.memory_item_id {
        Some(memory_item_id) => get_memory_item_by_id(client, memory_item_id).await.ok(),
        None => None,
    };
    let memory_item_present = record.memory_item_id.is_some();
    let memory_item_found = memory_item.is_some();
    let memory_item_scope_matches = !memory_item_present
        || memory_item.as_ref().is_some_and(|item| {
            item.project_code == project.code
                && item.namespace_code.as_deref() == Some(namespace.code.as_str())
        });
    let artifact_ref = match record.artifact_ref_id {
        Some(artifact_ref_id) => get_artifact_ref(client, artifact_ref_id).await.ok(),
        None => None,
    };
    let artifact_ref_present = record.artifact_ref_id.is_some();
    let artifact_ref_found = artifact_ref.is_some();
    let artifact_ref_scope_matches = !artifact_ref_present
        || artifact_ref.as_ref().is_some_and(|artifact| {
            artifact.project_code == project.code && artifact.namespace_code == namespace.code
        });
    let source_snapshot = match record.source_snapshot_id {
        Some(snapshot_id) => get_observability_snapshot_record(client, &snapshot_id).await?,
        None => None,
    };
    let source_snapshot_present = record.source_snapshot_id.is_some();
    let source_snapshot_found = source_snapshot.is_some();
    let source_snapshot_scope_matches = !source_snapshot_present
        || source_snapshot.as_ref().is_some_and(|snapshot| {
            let (snapshot_project_code, snapshot_namespace_code) =
                extract_restore_snapshot_scope(&snapshot.payload);
            snapshot_project_code
                .as_deref()
                .is_none_or(|code| code == project.code)
                && snapshot_namespace_code
                    .as_deref()
                    .is_none_or(|code| code == namespace.code)
        });
    let scope_binding_valid = (!memory_item_present
        || (memory_item_found && memory_item_scope_matches))
        && (!artifact_ref_present || (artifact_ref_found && artifact_ref_scope_matches))
        && (!source_snapshot_present || (source_snapshot_found && source_snapshot_scope_matches));
    Ok(MemoryProvenancePolicyScopeFilter {
        workspace_code: workspace_code.to_string(),
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        memory_item_present,
        memory_item_found,
        memory_item_scope_matches,
        artifact_ref_present,
        artifact_ref_found,
        artifact_ref_scope_matches,
        source_snapshot_present,
        source_snapshot_found,
        source_snapshot_scope_matches,
        scope_binding_valid,
    })
}

pub(super) fn validate_memory_provenance_policy_scope_filter(
    filter: &MemoryProvenancePolicyScopeFilter,
) -> Result<()> {
    if filter.memory_item_present && !filter.memory_item_found {
        return Err(anyhow!("memory provenance references missing memory item"));
    }
    if !filter.memory_item_scope_matches {
        return Err(anyhow!(
            "memory provenance memory item scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if filter.artifact_ref_present && !filter.artifact_ref_found {
        return Err(anyhow!("memory provenance references missing artifact ref"));
    }
    if !filter.artifact_ref_scope_matches {
        return Err(anyhow!(
            "memory provenance artifact ref scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if filter.source_snapshot_present && !filter.source_snapshot_found {
        return Err(anyhow!(
            "memory provenance references missing source snapshot"
        ));
    }
    if !filter.source_snapshot_scope_matches {
        return Err(anyhow!(
            "memory provenance source snapshot scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.scope_binding_valid {
        return Err(anyhow!(
            "memory provenance failed policy/scope filter before truth write"
        ));
    }
    Ok(())
}

pub(super) fn run_memory_provenance_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    scope_filter: &MemoryProvenancePolicyScopeFilter,
) -> MemoryProvenanceVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || !source_event_ids.as_array().unwrap_or(&vec![]).is_empty()
        || !artifact_refs.as_array().unwrap_or(&vec![]).is_empty()
        || !message_refs.as_array().unwrap_or(&vec![]).is_empty()
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty());
    let poisoned_detected = memory_provenance_marks_poisoned(evidence_span);
    let write_allowed = evidence_present && !poisoned_detected && scope_filter.scope_binding_valid;
    MemoryProvenanceVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        write_allowed,
    }
}

pub(super) fn validate_memory_provenance_verification_conflict_check(
    check: &MemoryProvenanceVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "memory provenance is flagged poisoned by evidence span and cannot be written"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "memory provenance must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "memory provenance failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

pub(super) fn augment_memory_provenance_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &MemoryProvenancePolicyScopeFilter,
    verification_check: &MemoryProvenanceVerificationConflictCheck,
) -> Value {
    let mut object = match evidence_span {
        Value::Object(map) => map.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("user_evidence_span".to_string(), evidence_span.clone());
            fallback
        }
    };
    object.insert(
        "stage2_runtime".to_string(),
        json!({
            "policy_and_scope_filter": {
                "workspace_code": policy_filter.workspace_code,
                "project_code": policy_filter.project_code,
                "namespace_code": policy_filter.namespace_code,
                "memory_item_present": policy_filter.memory_item_present,
                "memory_item_found": policy_filter.memory_item_found,
                "memory_item_scope_matches": policy_filter.memory_item_scope_matches,
                "artifact_ref_present": policy_filter.artifact_ref_present,
                "artifact_ref_found": policy_filter.artifact_ref_found,
                "artifact_ref_scope_matches": policy_filter.artifact_ref_scope_matches,
                "source_snapshot_present": policy_filter.source_snapshot_present,
                "source_snapshot_found": policy_filter.source_snapshot_found,
                "source_snapshot_scope_matches": policy_filter.source_snapshot_scope_matches,
                "scope_binding_valid": policy_filter.scope_binding_valid,
            },
            "verification_conflict_check": {
                "evidence_present": verification_check.evidence_present,
                "poisoned_detected": verification_check.poisoned_detected,
                "write_allowed": verification_check.write_allowed,
            }
        }),
    );
    Value::Object(object)
}

pub(super) async fn run_retrieval_trace_policy_scope_filter(
    client: &Client,
    workspace_code: &str,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &RetrievalTraceInsert,
) -> Result<RetrievalTracePolicyScopeFilter> {
    let context_pack_present = record.context_pack_id.is_some();
    let context_pack = match record.context_pack_id {
        Some(context_pack_id) => get_context_pack(client, context_pack_id).await.ok(),
        None => None,
    };
    let context_pack_found = context_pack.is_some();
    let context_pack_project_code = context_pack.as_ref().map(|item| item.project_code.clone());
    let context_pack_namespace_code = context_pack
        .as_ref()
        .map(|item| item.namespace_code.clone());
    let context_pack_scope_matches = !context_pack_present
        || (context_pack_project_code.as_deref() == Some(project.code.as_str())
            && context_pack_namespace_code.as_deref() == Some(namespace.code.as_str()));
    let scope_binding_valid =
        !context_pack_present || (context_pack_found && context_pack_scope_matches);
    Ok(RetrievalTracePolicyScopeFilter {
        workspace_code: workspace_code.to_string(),
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        context_pack_present,
        context_pack_found,
        context_pack_project_code,
        context_pack_namespace_code,
        context_pack_scope_matches,
        scope_binding_valid,
    })
}

pub(super) fn validate_retrieval_trace_policy_scope_filter(
    filter: &RetrievalTracePolicyScopeFilter,
) -> Result<()> {
    if filter.context_pack_present && !filter.context_pack_found {
        return Err(anyhow!("retrieval trace references missing context pack"));
    }
    if !filter.context_pack_scope_matches {
        return Err(anyhow!(
            "retrieval trace context pack scope {:?}:{:?} does not match target {}:{}",
            filter.context_pack_project_code,
            filter.context_pack_namespace_code,
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.scope_binding_valid {
        return Err(anyhow!(
            "retrieval trace failed policy/scope filter before truth write"
        ));
    }
    Ok(())
}

pub(super) fn run_retrieval_trace_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    evidence_sufficiency: &Value,
    trace_payload: &Value,
    scope_filter: &RetrievalTracePolicyScopeFilter,
) -> RetrievalTraceVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || !source_event_ids.as_array().unwrap_or(&vec![]).is_empty()
        || !artifact_refs.as_array().unwrap_or(&vec![]).is_empty()
        || !message_refs.as_array().unwrap_or(&vec![]).is_empty()
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty());
    let poisoned_detected = retrieval_trace_marks_poisoned(evidence_span);
    let sufficiency_layer = evidence_sufficiency
        .get("cheapest_sufficient_layer")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let decision_trace_layer = trace_payload
        .get("decision_trace")
        .and_then(Value::as_object)
        .and_then(|trace| trace.get("cheapest_sufficient_layer"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let sufficiency_basis_present = sufficiency_layer.is_some()
        || evidence_sufficiency
            .as_object()
            .is_some_and(|obj| !obj.is_empty());
    let decision_trace_consistent = match (sufficiency_layer, decision_trace_layer) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    };
    let write_allowed = evidence_present
        && !poisoned_detected
        && sufficiency_basis_present
        && decision_trace_consistent
        && scope_filter.scope_binding_valid;
    RetrievalTraceVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        sufficiency_basis_present,
        decision_trace_consistent,
        write_allowed,
    }
}

pub(super) fn validate_retrieval_trace_verification_conflict_check(
    check: &RetrievalTraceVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "retrieval trace is flagged poisoned by evidence span and cannot be written"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "retrieval trace must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.sufficiency_basis_present {
        return Err(anyhow!(
            "retrieval trace must carry evidence sufficiency basis before truth write"
        ));
    }
    if !check.decision_trace_consistent {
        return Err(anyhow!(
            "retrieval trace cheapest sufficient layer disagrees between evidence_sufficiency and decision_trace"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "retrieval trace failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

pub(super) fn augment_retrieval_trace_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &RetrievalTracePolicyScopeFilter,
    verification_check: &RetrievalTraceVerificationConflictCheck,
) -> Value {
    let mut object = match evidence_span {
        Value::Object(map) => map.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("user_evidence_span".to_string(), evidence_span.clone());
            fallback
        }
    };
    object.insert(
        "stage2_runtime".to_string(),
        json!({
            "policy_and_scope_filter": {
                "workspace_code": policy_filter.workspace_code,
                "project_code": policy_filter.project_code,
                "namespace_code": policy_filter.namespace_code,
                "context_pack_present": policy_filter.context_pack_present,
                "context_pack_found": policy_filter.context_pack_found,
                "context_pack_project_code": policy_filter.context_pack_project_code,
                "context_pack_namespace_code": policy_filter.context_pack_namespace_code,
                "context_pack_scope_matches": policy_filter.context_pack_scope_matches,
                "scope_binding_valid": policy_filter.scope_binding_valid,
            },
            "verification_conflict_check": {
                "evidence_present": verification_check.evidence_present,
                "poisoned_detected": verification_check.poisoned_detected,
                "sufficiency_basis_present": verification_check.sufficiency_basis_present,
                "decision_trace_consistent": verification_check.decision_trace_consistent,
                "write_allowed": verification_check.write_allowed,
            }
        }),
    );
    Value::Object(object)
}

fn extract_restore_snapshot_scope(payload: &Value) -> (Option<String>, Option<String>) {
    let root = payload
        .get("working_state_restore")
        .filter(|value| value.is_object())
        .unwrap_or(payload);
    let project_code = root
        .get("project")
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let namespace_code = root
        .get("namespace")
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    (project_code, namespace_code)
}

pub(super) async fn run_restore_pack_policy_scope_filter(
    client: &Client,
    workspace_code: &str,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &RestorePackInsert<'_>,
) -> Result<RestorePackPolicyScopeFilter> {
    let source_snapshot_required = record.pack_kind == "workspace_restore_pack";
    let source_snapshot_present = record.source_snapshot_id.is_some();
    let (
        source_snapshot_found,
        source_snapshot_kind,
        source_snapshot_project_code,
        source_snapshot_namespace_code,
    ) = if let Some(hint) = record.source_snapshot_hint.as_ref() {
        (
            hint.verified_exists,
            Some(hint.snapshot_kind.to_string()),
            hint.scope_project_code.map(ToOwned::to_owned),
            hint.scope_namespace_code.map(ToOwned::to_owned),
        )
    } else {
        let snapshot = match record.source_snapshot_id {
            Some(snapshot_id) => get_observability_snapshot_record(client, &snapshot_id).await?,
            None => None,
        };
        let source_snapshot_found = snapshot.is_some();
        let source_snapshot_kind = snapshot.as_ref().map(|item| item.snapshot_kind.clone());
        let (source_snapshot_project_code, source_snapshot_namespace_code) = snapshot
            .as_ref()
            .map(|item| extract_restore_snapshot_scope(&item.payload))
            .unwrap_or((None, None));
        (
            source_snapshot_found,
            source_snapshot_kind,
            source_snapshot_project_code,
            source_snapshot_namespace_code,
        )
    };
    let snapshot_kind_valid = match (record.pack_kind, source_snapshot_kind.as_deref()) {
        ("workspace_restore_pack", Some("working_state_restore")) => true,
        ("workspace_restore_pack", Some(_)) => false,
        (_, Some(_)) => true,
        (_, None) => !source_snapshot_present,
    };
    let snapshot_project_matches = source_snapshot_project_code
        .as_deref()
        .is_none_or(|code| code == project.code);
    let snapshot_namespace_matches = source_snapshot_namespace_code
        .as_deref()
        .is_none_or(|code| code == namespace.code);
    let snapshot_scope_matches =
        !source_snapshot_present || (snapshot_project_matches && snapshot_namespace_matches);
    let scope_binding_valid = (!source_snapshot_required || source_snapshot_present)
        && (!source_snapshot_present || source_snapshot_found)
        && snapshot_kind_valid
        && snapshot_scope_matches;
    Ok(RestorePackPolicyScopeFilter {
        workspace_code: workspace_code.to_string(),
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        pack_kind: record.pack_kind.to_string(),
        source_snapshot_required,
        source_snapshot_present,
        source_snapshot_found,
        source_snapshot_kind,
        source_snapshot_project_code,
        source_snapshot_namespace_code,
        snapshot_kind_valid,
        snapshot_scope_matches,
        scope_binding_valid,
    })
}

pub(super) fn validate_restore_pack_policy_scope_filter(
    filter: &RestorePackPolicyScopeFilter,
) -> Result<()> {
    if filter.source_snapshot_required && !filter.source_snapshot_present {
        return Err(anyhow!(
            "restore pack {} requires source_snapshot_id",
            filter.pack_kind
        ));
    }
    if filter.source_snapshot_present && !filter.source_snapshot_found {
        return Err(anyhow!(
            "restore pack {} references missing source snapshot",
            filter.pack_kind
        ));
    }
    if !filter.snapshot_kind_valid {
        return Err(anyhow!(
            "restore pack {} requires compatible source snapshot kind, got {:?}",
            filter.pack_kind,
            filter.source_snapshot_kind
        ));
    }
    if !filter.snapshot_scope_matches {
        return Err(anyhow!(
            "restore pack {} source snapshot scope {:?}:{:?} does not match target {}:{}",
            filter.pack_kind,
            filter.source_snapshot_project_code,
            filter.source_snapshot_namespace_code,
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.scope_binding_valid {
        return Err(anyhow!(
            "restore pack {} failed policy/scope filter before truth write",
            filter.pack_kind
        ));
    }
    Ok(())
}

pub(super) fn run_restore_pack_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    scope_filter: &RestorePackPolicyScopeFilter,
) -> RestorePackVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || !source_event_ids.as_array().unwrap_or(&vec![]).is_empty()
        || !artifact_refs.as_array().unwrap_or(&vec![]).is_empty()
        || !message_refs.as_array().unwrap_or(&vec![]).is_empty()
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty());
    let poisoned_detected = restore_pack_marks_poisoned(evidence_span);
    let write_allowed = evidence_present && !poisoned_detected && scope_filter.scope_binding_valid;
    RestorePackVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        write_allowed,
    }
}

pub(super) fn validate_restore_pack_verification_conflict_check(
    check: &RestorePackVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "restore pack is flagged poisoned by evidence span and cannot be written"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "restore pack must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "restore pack failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

pub(super) fn augment_restore_pack_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &RestorePackPolicyScopeFilter,
    verification_check: &RestorePackVerificationConflictCheck,
) -> Value {
    let mut object = match evidence_span {
        Value::Object(map) => map.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("user_evidence_span".to_string(), evidence_span.clone());
            fallback
        }
    };
    object.insert(
        "stage2_runtime".to_string(),
        json!({
            "policy_and_scope_filter": {
                "workspace_code": policy_filter.workspace_code,
                "project_code": policy_filter.project_code,
                "namespace_code": policy_filter.namespace_code,
                "pack_kind": policy_filter.pack_kind,
                "source_snapshot_required": policy_filter.source_snapshot_required,
                "source_snapshot_present": policy_filter.source_snapshot_present,
                "source_snapshot_found": policy_filter.source_snapshot_found,
                "source_snapshot_kind": policy_filter.source_snapshot_kind,
                "source_snapshot_project_code": policy_filter.source_snapshot_project_code,
                "source_snapshot_namespace_code": policy_filter.source_snapshot_namespace_code,
                "snapshot_kind_valid": policy_filter.snapshot_kind_valid,
                "snapshot_scope_matches": policy_filter.snapshot_scope_matches,
                "scope_binding_valid": policy_filter.scope_binding_valid,
            },
            "verification_conflict_check": {
                "evidence_present": verification_check.evidence_present,
                "poisoned_detected": verification_check.poisoned_detected,
                "write_allowed": verification_check.write_allowed,
            }
        }),
    );
    Value::Object(object)
}

pub(super) fn extract_skill_card_candidate(
    skill_source_event_ids: &[String],
    skill_artifact_refs: &[String],
    skill_evidence_span: &Value,
    skill_candidate_class: Option<&str>,
    skill_title: Option<&str>,
    skill_derivation_kind: Option<&str>,
) -> SkillCardCandidateExtraction {
    let source_event_count = skill_source_event_ids.len();
    let artifact_ref_count = skill_artifact_refs.len();
    let has_evidence_span = skill_evidence_span
        .as_object()
        .is_some_and(|span| !span.is_empty());
    let source_basis_status =
        if source_event_count > 0 || artifact_ref_count > 0 || has_evidence_span {
            "recorded"
        } else {
            "missing"
        }
        .to_string();
    let derivation_kind = skill_derivation_kind.unwrap_or("extract").to_string();
    let candidate_class = canonical_candidate_class_from_hints(
        skill_candidate_class,
        Some("skill_hint"),
        skill_title,
        &[],
        false,
        "skill_hint",
    );
    let source_kind = if source_event_count > 0 {
        Some("raw_event_append".to_string())
    } else if artifact_ref_count > 0 {
        Some("artifact_basis".to_string())
    } else if has_evidence_span {
        Some("evidence_span_basis".to_string())
    } else {
        None
    };
    let (hot_path_write_eligible, background_consolidation_recommended) =
        runtime_contract_for_candidate_class(&candidate_class, &derivation_kind);
    SkillCardCandidateExtraction {
        source_basis_status,
        source_event_count,
        artifact_ref_count,
        has_evidence_span,
        candidate_class,
        derivation_kind,
        source_kind,
        hot_path_write_eligible,
        background_consolidation_recommended,
    }
}

pub(super) fn validate_skill_card_candidate(
    candidate: &SkillCardCandidateExtraction,
) -> Result<()> {
    if candidate.derivation_kind != "operator_write" && candidate.source_basis_status != "recorded"
    {
        return Err(anyhow!(
            "skill card candidate requires recorded provenance basis unless derivation_kind=operator_write"
        ));
    }
    Ok(())
}

pub(super) fn run_skill_card_policy_scope_filter(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    skill_owner_scope: &str,
) -> SkillCardPolicyScopeFilter {
    let visibility_scope = project.visibility_scope.clone();
    let owner_agent_required = visibility_scope == "agent_private";
    let owner_agent_present = skill_owner_scope
        .strip_prefix("agent:")
        .is_some_and(|value| !value.trim().is_empty());
    let private_contour_violation = owner_agent_required && !owner_agent_present;
    let scope_allowed = !private_contour_violation;
    SkillCardPolicyScopeFilter {
        visibility_scope,
        skill_owner_scope: skill_owner_scope.to_string(),
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        owner_agent_required,
        owner_agent_present,
        private_contour_violation,
        scope_allowed,
    }
}

pub(super) fn validate_skill_card_policy_scope_filter(
    filter: &SkillCardPolicyScopeFilter,
) -> Result<()> {
    if filter.visibility_scope == "quarantine" {
        return Err(anyhow!(
            "skill card violates scope filter: visibility_scope=quarantine requires dedicated quarantine_item path"
        ));
    }
    if !filter.scope_allowed {
        return Err(anyhow!(
            "skill card violates scope filter: visibility_scope={} requires agent-bound skill_owner_scope",
            filter.visibility_scope
        ));
    }
    Ok(())
}

pub(super) fn evidence_span_marks_skill_card_poisoned(skill_evidence_span: &Value) -> bool {
    skill_evidence_span
        .get("poisoned")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || skill_evidence_span
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

async fn run_skill_card_verification_conflict_check(
    client: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    candidate: &SkillCardCandidateExtraction,
    skill_id: &str,
    skill_version: i32,
    skill_evidence_span: &Value,
    scope_filter: &SkillCardPolicyScopeFilter,
) -> Result<SkillCardVerificationConflictCheck> {
    let evidence_present = candidate.derivation_kind == "operator_write"
        || candidate.source_basis_status == "recorded";
    let poisoned_detected = evidence_span_marks_skill_card_poisoned(skill_evidence_span);
    let duplicate_version_conflict = client
        .query_one(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM ami.skill_cards sc
                WHERE sc.project_id = $1
                  AND sc.namespace_id = $2
                  AND sc.skill_id = $3
                  AND sc.skill_version = $4
            )
            "#,
            &[
                &project.project_id,
                &namespace.namespace_id,
                &skill_id,
                &skill_version,
            ],
        )
        .await
        .context("failed to check skill card duplicate version conflict")?
        .get::<_, bool>(0);
    let write_allowed = evidence_present
        && !poisoned_detected
        && !duplicate_version_conflict
        && !scope_filter.private_contour_violation;
    Ok(SkillCardVerificationConflictCheck {
        evidence_present,
        duplicate_version_conflict,
        poisoned_detected,
        private_contour_violation: scope_filter.private_contour_violation,
        skill_id: skill_id.to_string(),
        skill_version,
        write_allowed,
    })
}

pub(super) fn validate_skill_card_verification_conflict_check(
    check: &SkillCardVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "skill card is flagged poisoned by evidence span and cannot be written"
        ));
    }
    if check.duplicate_version_conflict {
        return Err(anyhow!(
            "skill card conflicts with existing skill_id/version truth in the same namespace"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "skill card must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "skill card failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

fn augment_skill_evidence_span_with_stage2_preflight(
    skill_evidence_span: &Value,
    candidate: &SkillCardCandidateExtraction,
    scope_filter: &SkillCardPolicyScopeFilter,
    verification_check: &SkillCardVerificationConflictCheck,
) -> Value {
    let mut object = match skill_evidence_span {
        Value::Object(map) => map.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert(
                "user_evidence_span".to_string(),
                skill_evidence_span.clone(),
            );
            fallback
        }
    };
    object.insert(
        "stage2_runtime".to_string(),
        json!({
            "candidate_class": candidate.candidate_class,
            "source_kind": candidate.source_kind,
            "source_basis_status": candidate.source_basis_status,
            "hot_path_write_eligible": candidate.hot_path_write_eligible,
            "background_consolidation_recommended": candidate.background_consolidation_recommended,
            "policy_and_scope_filter": {
                "visibility_scope": scope_filter.visibility_scope,
                "skill_owner_scope": scope_filter.skill_owner_scope,
                "project_code": scope_filter.project_code,
                "namespace_code": scope_filter.namespace_code,
                "owner_agent_required": scope_filter.owner_agent_required,
                "owner_agent_present": scope_filter.owner_agent_present,
                "private_contour_violation": scope_filter.private_contour_violation,
                "scope_allowed": scope_filter.scope_allowed,
            },
            "verification_conflict_check": {
                "evidence_present": verification_check.evidence_present,
                "duplicate_version_conflict": verification_check.duplicate_version_conflict,
                "poisoned_detected": verification_check.poisoned_detected,
                "private_contour_violation": verification_check.private_contour_violation,
                "skill_id": verification_check.skill_id,
                "skill_version": verification_check.skill_version,
                "write_allowed": verification_check.write_allowed,
            }
        }),
    );
    Value::Object(object)
}

pub(super) fn validate_skill_evidence_bundle_basis(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
) -> Result<()> {
    let has_source_event_ids = value_string_array_len(Some(source_event_ids)) > 0;
    let has_artifact_refs = value_string_array_len(Some(artifact_refs)) > 0;
    let has_message_refs = value_string_array_len(Some(message_refs)) > 0;
    let has_evidence_span = evidence_span
        .as_object()
        .map(|object| !object.is_empty())
        .unwrap_or(false);
    if derivation_kind != "operator_write"
        && !has_source_event_ids
        && !has_artifact_refs
        && !has_message_refs
        && !has_evidence_span
    {
        return Err(anyhow!(
            "skill evidence bundle requires recorded basis unless derivation_kind=operator_write"
        ));
    }
    Ok(())
}

pub(super) fn validate_skill_activity_basis(
    surface_name: &str,
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
) -> Result<()> {
    let has_source_event_ids = value_string_array_len(Some(source_event_ids)) > 0;
    let has_artifact_refs = value_string_array_len(Some(artifact_refs)) > 0;
    let has_message_refs = value_string_array_len(Some(message_refs)) > 0;
    let has_evidence_span = evidence_span
        .as_object()
        .map(|object| !object.is_empty())
        .unwrap_or(false);
    if derivation_kind != "operator_write"
        && !has_source_event_ids
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

#[cfg(test)]
pub async fn create_skill_card_candidate(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    skill_id: &str,
    skill_version: i32,
    skill_title: &str,
    skill_goal: &str,
    skill_trigger_conditions: &[String],
    skill_preconditions: &[String],
    skill_execution_steps: &[String],
    skill_stop_conditions: &[String],
    skill_forbidden_when: &[String],
    skill_expected_outcome: Option<&str>,
    skill_scope_type: &str,
    skill_owner_scope: &str,
    skill_runtime_constraints: &[String],
    skill_model_constraints: &[String],
    skill_tool_constraints: &[String],
    skill_context_constraints: &[String],
    skill_source_event_ids: &[String],
    skill_artifact_refs: &[String],
    skill_evidence_span: &Value,
    skill_candidate_class: Option<&str>,
    skill_derivation_kind: Option<&str>,
) -> Result<SkillCardRecord> {
    create_skill_card_candidate_with_refinement(
        client,
        project_code,
        namespace_code,
        skill_id,
        skill_version,
        skill_title,
        skill_goal,
        skill_trigger_conditions,
        skill_preconditions,
        skill_execution_steps,
        skill_stop_conditions,
        skill_forbidden_when,
        skill_expected_outcome,
        skill_scope_type,
        skill_owner_scope,
        skill_runtime_constraints,
        skill_model_constraints,
        skill_tool_constraints,
        skill_context_constraints,
        skill_source_event_ids,
        skill_artifact_refs,
        skill_evidence_span,
        skill_candidate_class,
        None,
        None,
        None,
        skill_derivation_kind,
    )
    .await
}

pub async fn create_skill_card_candidate_with_refinement(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    skill_id: &str,
    skill_version: i32,
    skill_title: &str,
    skill_goal: &str,
    skill_trigger_conditions: &[String],
    skill_preconditions: &[String],
    skill_execution_steps: &[String],
    skill_stop_conditions: &[String],
    skill_forbidden_when: &[String],
    skill_expected_outcome: Option<&str>,
    skill_scope_type: &str,
    skill_owner_scope: &str,
    skill_runtime_constraints: &[String],
    skill_model_constraints: &[String],
    skill_tool_constraints: &[String],
    skill_context_constraints: &[String],
    skill_source_event_ids: &[String],
    skill_artifact_refs: &[String],
    skill_evidence_span: &Value,
    skill_candidate_class: Option<&str>,
    skill_refinement_action: Option<&str>,
    skill_patch_parent_skill_card_id: Option<Uuid>,
    skill_merge_group_id: Option<Uuid>,
    skill_derivation_kind: Option<&str>,
) -> Result<SkillCardRecord> {
    let project = get_project_by_code(client, project_code).await?;
    let namespace = get_namespace_by_code(client, project.project_id, namespace_code).await?;
    let workspace_row = client
        .query_one(
            r#"
            SELECT w.workspace_id, w.code
            FROM ami.projects p
            INNER JOIN ami.workspaces w ON w.workspace_id = p.workspace_id
            WHERE p.project_id = $1
            "#,
            &[&project.project_id],
        )
        .await
        .context("failed to load workspace context for skill candidate")?;
    let workspace_id: Uuid = workspace_row.get(0);
    let workspace_code: String = workspace_row.get(1);
    let candidate = extract_skill_card_candidate(
        skill_source_event_ids,
        skill_artifact_refs,
        skill_evidence_span,
        skill_candidate_class,
        Some(skill_title),
        skill_derivation_kind,
    );
    validate_skill_card_candidate(&candidate)?;
    let scope_filter = run_skill_card_policy_scope_filter(&project, &namespace, skill_owner_scope);
    validate_skill_card_policy_scope_filter(&scope_filter)?;
    let verification_check = run_skill_card_verification_conflict_check(
        client,
        &project,
        &namespace,
        &candidate,
        skill_id,
        skill_version,
        skill_evidence_span,
        &scope_filter,
    )
    .await?;
    validate_skill_card_verification_conflict_check(&verification_check)?;
    let refinement_decision = decide_skill_refinement(
        client,
        &project,
        &namespace,
        skill_id,
        skill_version,
        skill_title,
        skill_goal,
        &candidate.candidate_class,
        skill_scope_type,
        skill_owner_scope,
        skill_runtime_constraints,
        skill_model_constraints,
        skill_tool_constraints,
        skill_context_constraints,
        skill_trigger_conditions,
        skill_execution_steps,
        skill_refinement_action,
        skill_patch_parent_skill_card_id,
        skill_merge_group_id,
    )
    .await?;
    let stored_skill_evidence_span = augment_skill_evidence_span_with_stage2_preflight(
        skill_evidence_span,
        &candidate,
        &scope_filter,
        &verification_check,
    )
    .as_object()
    .cloned()
    .map(|mut span| {
        span.insert(
            "skill_refinement_decision".to_string(),
            serde_json::to_value(&refinement_decision).unwrap_or_else(|_| json!({})),
        );
        Value::Object(span)
    })
    .unwrap_or_else(|| json!({"skill_refinement_decision": refinement_decision}));
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.skill_cards(
                workspace_id,
                project_id,
                namespace_id,
                skill_id,
                skill_version,
                skill_title,
                skill_goal,
                skill_trigger_conditions,
                skill_preconditions,
                skill_execution_steps,
                skill_stop_conditions,
                skill_forbidden_when,
                skill_expected_outcome,
                skill_scope_type,
                skill_owner_scope,
                skill_trust_state,
                skill_verification_state,
                skill_runtime_constraints,
                skill_model_constraints,
                skill_tool_constraints,
                skill_context_constraints,
                skill_source_event_ids,
                skill_artifact_refs,
                skill_evidence_span,
                skill_candidate_class,
                skill_derivation_kind,
                skill_source_kind,
                skill_patch_parent_id,
                skill_merge_group_id,
                skill_shared_promotion_state,
                skill_hot_path_write_eligible,
                skill_background_consolidation_recommended
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8::jsonb, $9::jsonb, $10::jsonb, $11::jsonb, $12::jsonb,
                $13, $14, $15, 'candidate', 'unverified',
                $16::jsonb, $17::jsonb, $18::jsonb, $19::jsonb, $20::jsonb, $21::jsonb,
                $22::jsonb, $23, $24, $25, $26, $27,
                CASE WHEN $14 = 'project_shared' THEN 'pending_approval' ELSE 'not_applicable' END,
                $28, $29
            )
            RETURNING
                skill_card_id,
                $30::text,
                $31::text,
                $32::text,
                skill_id,
                skill_version,
                skill_title,
                skill_goal,
                skill_trigger_conditions,
                skill_preconditions,
                skill_execution_steps,
                skill_stop_conditions,
                skill_forbidden_when,
                skill_expected_outcome,
                skill_scope_type,
                skill_owner_scope,
                skill_trust_state,
                skill_verification_state,
                skill_runtime_constraints,
                skill_model_constraints,
                skill_tool_constraints,
                skill_context_constraints,
                skill_source_event_ids,
                skill_artifact_refs,
                skill_evidence_span,
                skill_candidate_class,
                skill_derivation_kind,
                skill_source_kind,
                skill_hot_path_write_eligible,
                skill_background_consolidation_recommended,
                skill_success_count,
                skill_failure_count,
                skill_reuse_count,
                skill_shadow_pass_count,
                skill_shadow_fail_count,
                to_char(skill_last_used_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                to_char(skill_last_verified_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                skill_patch_parent_id,
                skill_merge_group_id,
                skill_shared_promotion_state,
                skill_shared_approved_by,
                skill_shared_approval_reason,
                to_char(skill_shared_approved_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                skill_utility_score
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &skill_id,
                &skill_version,
                &skill_title,
                &skill_goal,
                &string_array_json(skill_trigger_conditions),
                &string_array_json(skill_preconditions),
                &string_array_json(skill_execution_steps),
                &string_array_json(skill_stop_conditions),
                &string_array_json(skill_forbidden_when),
                &skill_expected_outcome,
                &skill_scope_type,
                &skill_owner_scope,
                &string_array_json(skill_runtime_constraints),
                &string_array_json(skill_model_constraints),
                &string_array_json(skill_tool_constraints),
                &string_array_json(skill_context_constraints),
                &string_array_json(skill_source_event_ids),
                &string_array_json(skill_artifact_refs),
                &stored_skill_evidence_span,
                &candidate.candidate_class,
                &candidate.derivation_kind,
                &candidate.source_kind,
                &refinement_decision.patch_parent_skill_card_id,
                &refinement_decision.merge_group_id,
                &candidate.hot_path_write_eligible,
                &candidate.background_consolidation_recommended,
                &workspace_code,
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| format!("failed to create skill candidate {skill_id}@v{skill_version}"))?;
    Ok(skill_card_record_from_row(&row))
}

pub async fn create_skill_evidence_bundle(
    client: &Client,
    skill_card_id: Uuid,
    evidence_kind: &str,
    summary: Option<&str>,
    source_event_ids: &[String],
    artifact_refs: &[String],
    source_kind: Option<&str>,
    message_refs: Option<&Value>,
    evidence_span: Option<&Value>,
    derivation_kind: Option<&str>,
    schema_version: Option<&str>,
) -> Result<SkillEvidenceBundleRecord> {
    let source_event_ids = string_array_json(source_event_ids);
    let artifact_refs = string_array_json(artifact_refs);
    let message_refs = message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = derivation_kind.unwrap_or("extract");
    let schema_version = schema_version.unwrap_or("skill-evidence-bundle-envelope-v1");
    validate_skill_evidence_bundle_basis(
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.skill_evidence_bundles(
                skill_card_id,
                evidence_kind,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            )
            VALUES ($1, $2, $3, $4, $5::jsonb, $6::jsonb, $7::jsonb, $8::jsonb, $9, $10)
            RETURNING
                skill_evidence_bundle_id,
                skill_card_id,
                evidence_kind,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            "#,
            &[
                &skill_card_id,
                &evidence_kind,
                &summary,
                &source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &evidence_span,
                &derivation_kind,
                &schema_version,
            ],
        )
        .await
        .with_context(|| format!("failed to create skill evidence bundle for {skill_card_id}"))?;
    client
        .execute(
            r#"
            UPDATE ami.skill_cards
            SET skill_verification_state = CASE
                    WHEN skill_verification_state = 'unverified' THEN 'evidence_attached'
                    ELSE skill_verification_state
                END,
                updated_at = now()
            WHERE skill_card_id = $1
            "#,
            &[&skill_card_id],
        )
        .await
        .context("failed to bump skill verification state after evidence")?;
    Ok(SkillEvidenceBundleRecord {
        skill_evidence_bundle_id: row.get(0),
        skill_card_id: row.get(1),
        evidence_kind: row.get(2),
        summary: row.get(3),
        source_kind: row.get(4),
        source_event_ids: row.get(5),
        artifact_refs: row.get(6),
        message_refs: row.get(7),
        evidence_span: row.get(8),
        derivation_kind: row.get(9),
        schema_version: row.get(10),
    })
}

pub async fn get_skill_evidence_bundle(
    client: &Client,
    skill_evidence_bundle_id: Uuid,
) -> Result<SkillEvidenceBundleRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                skill_evidence_bundle_id,
                skill_card_id,
                evidence_kind,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            FROM ami.skill_evidence_bundles
            WHERE skill_evidence_bundle_id = $1
            "#,
            &[&skill_evidence_bundle_id],
        )
        .await
        .with_context(|| {
            format!(
                "failed to load skill evidence bundle {}",
                skill_evidence_bundle_id
            )
        })?;
    Ok(SkillEvidenceBundleRecord {
        skill_evidence_bundle_id: row.get(0),
        skill_card_id: row.get(1),
        evidence_kind: row.get(2),
        summary: row.get(3),
        source_kind: row.get(4),
        source_event_ids: row.get(5),
        artifact_refs: row.get(6),
        message_refs: row.get(7),
        evidence_span: row.get(8),
        derivation_kind: row.get(9),
        schema_version: row.get(10),
    })
}

pub async fn record_skill_trigger_match(
    client: &Client,
    skill_card_id: Uuid,
    match_scope: &str,
    trigger_input: &str,
    matched: bool,
    summary: Option<&str>,
    source_kind: Option<&str>,
    source_event_ids: Option<&Value>,
    artifact_refs: Option<&Value>,
    message_refs: Option<&Value>,
    evidence_span: Option<&Value>,
    derivation_kind: Option<&str>,
    schema_version: Option<&str>,
) -> Result<SkillTriggerMatchRecord> {
    let source_event_ids = source_event_ids.cloned().unwrap_or_else(|| json!([]));
    let artifact_refs = artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs = message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = derivation_kind.unwrap_or("extract");
    let schema_version = schema_version.unwrap_or("skill-trigger-match-envelope-v1");
    validate_skill_activity_basis(
        "skill trigger match",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.skill_trigger_matches(
                skill_card_id,
                match_scope,
                trigger_input,
                matched,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8::jsonb, $9::jsonb, $10::jsonb, $11, $12)
            RETURNING
                skill_trigger_match_id,
                skill_card_id,
                match_scope,
                matched,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            "#,
            &[
                &skill_card_id,
                &match_scope,
                &trigger_input,
                &matched,
                &summary,
                &source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &evidence_span,
                &derivation_kind,
                &schema_version,
            ],
        )
        .await
        .with_context(|| format!("failed to record skill trigger match for {skill_card_id}"))?;
    Ok(SkillTriggerMatchRecord {
        skill_trigger_match_id: row.get(0),
        skill_card_id: row.get(1),
        match_scope: row.get(2),
        matched: row.get(3),
        summary: row.get(4),
        source_kind: row.get(5),
        source_event_ids: row.get(6),
        artifact_refs: row.get(7),
        message_refs: row.get(8),
        evidence_span: row.get(9),
        derivation_kind: row.get(10),
        schema_version: row.get(11),
    })
}

pub async fn get_skill_trigger_match(
    client: &Client,
    skill_trigger_match_id: Uuid,
) -> Result<SkillTriggerMatchRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                skill_trigger_match_id,
                skill_card_id,
                match_scope,
                matched,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            FROM ami.skill_trigger_matches
            WHERE skill_trigger_match_id = $1
            "#,
            &[&skill_trigger_match_id],
        )
        .await
        .with_context(|| {
            format!(
                "failed to load skill trigger match {}",
                skill_trigger_match_id
            )
        })?;
    Ok(SkillTriggerMatchRecord {
        skill_trigger_match_id: row.get(0),
        skill_card_id: row.get(1),
        match_scope: row.get(2),
        matched: row.get(3),
        summary: row.get(4),
        source_kind: row.get(5),
        source_event_ids: row.get(6),
        artifact_refs: row.get(7),
        message_refs: row.get(8),
        evidence_span: row.get(9),
        derivation_kind: row.get(10),
        schema_version: row.get(11),
    })
}

pub async fn record_skill_trial_run(
    client: &Client,
    skill_card_id: Uuid,
    application_mode: &str,
    task_label: Option<&str>,
    runtime: Option<&str>,
    model: Option<&str>,
    tool: Option<&str>,
    matched: bool,
    applied: bool,
    outcome: &str,
    summary: Option<&str>,
    source_kind: Option<&str>,
    source_event_ids: Option<&Value>,
    artifact_refs: Option<&Value>,
    message_refs: Option<&Value>,
    evidence_span: Option<&Value>,
    derivation_kind: Option<&str>,
    schema_version: Option<&str>,
) -> Result<SkillTrialRunRecord> {
    let source_event_ids = source_event_ids.cloned().unwrap_or_else(|| json!([]));
    let artifact_refs = artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs = message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = derivation_kind.unwrap_or("extract");
    let schema_version = schema_version.unwrap_or("skill-trial-run-envelope-v1");
    validate_skill_activity_basis(
        "skill trial run",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    enforce_skill_runtime_model_tool_constraints(
        client,
        skill_card_id,
        &evidence_span,
        runtime,
        model,
        tool,
        "skill trial run",
    )
    .await?;
    enforce_skill_context_constraints(client, skill_card_id, &evidence_span, "skill trial run")
        .await?;
    if outcome != "neutral" && !matched {
        return Err(anyhow!(
            "skill {} trial run outcome requires matched trigger",
            skill_card_id
        ));
    }
    if outcome != "neutral" && application_mode != "shadow" && !applied {
        return Err(anyhow!(
            "skill {} trial run outcome requires applied=true for {}",
            skill_card_id,
            application_mode
        ));
    }
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.skill_trial_runs(
                skill_card_id,
                application_mode,
                task_label,
                runtime_name,
                model_name,
                tool_name,
                matched,
                applied,
                outcome,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::jsonb, $13::jsonb, $14::jsonb, $15::jsonb, $16, $17)
            RETURNING
                skill_trial_run_id,
                skill_card_id,
                application_mode,
                matched,
                applied,
                outcome,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            "#,
            &[
                &skill_card_id,
                &application_mode,
                &task_label,
                &runtime,
                &model,
                &tool,
                &matched,
                &applied,
                &outcome,
                &summary,
                &source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &evidence_span,
                &derivation_kind,
                &schema_version,
            ],
        )
        .await
        .with_context(|| format!("failed to record skill trial run for {skill_card_id}"))?;
    let (success_delta, failure_delta, shadow_pass_delta, shadow_fail_delta) =
        match (application_mode, outcome, matched, applied) {
            ("shadow", "success", true, _) => (0, 0, 1, 0),
            ("shadow", "failure", true, _) => (0, 0, 0, 1),
            (_, "success", true, true) => (1, 0, 0, 0),
            (_, "failure", true, true) => (0, 1, 0, 0),
            _ => (0, 0, 0, 0),
        };
    client
        .execute(
            r#"
            UPDATE ami.skill_cards
            SET skill_success_count = skill_success_count + $2,
                skill_failure_count = skill_failure_count + $3,
                skill_shadow_pass_count = skill_shadow_pass_count + $4,
                skill_shadow_fail_count = skill_shadow_fail_count + $5,
                updated_at = now()
            WHERE skill_card_id = $1
            "#,
            &[
                &skill_card_id,
                &success_delta,
                &failure_delta,
                &shadow_pass_delta,
                &shadow_fail_delta,
            ],
        )
        .await
        .context("failed to update skill counters after trial run")?;
    Ok(SkillTrialRunRecord {
        skill_trial_run_id: row.get(0),
        skill_card_id: row.get(1),
        application_mode: row.get(2),
        matched: row.get(3),
        applied: row.get(4),
        outcome: row.get(5),
        source_kind: row.get(6),
        source_event_ids: row.get(7),
        artifact_refs: row.get(8),
        message_refs: row.get(9),
        evidence_span: row.get(10),
        derivation_kind: row.get(11),
        schema_version: row.get(12),
    })
}

pub async fn get_skill_trial_run(
    client: &Client,
    skill_trial_run_id: Uuid,
) -> Result<SkillTrialRunRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                skill_trial_run_id,
                skill_card_id,
                application_mode,
                matched,
                applied,
                outcome,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            FROM ami.skill_trial_runs
            WHERE skill_trial_run_id = $1
            "#,
            &[&skill_trial_run_id],
        )
        .await
        .with_context(|| format!("failed to load skill trial run {}", skill_trial_run_id))?;
    Ok(SkillTrialRunRecord {
        skill_trial_run_id: row.get(0),
        skill_card_id: row.get(1),
        application_mode: row.get(2),
        matched: row.get(3),
        applied: row.get(4),
        outcome: row.get(5),
        source_kind: row.get(6),
        source_event_ids: row.get(7),
        artifact_refs: row.get(8),
        message_refs: row.get(9),
        evidence_span: row.get(10),
        derivation_kind: row.get(11),
        schema_version: row.get(12),
    })
}

pub async fn record_skill_eval(
    client: &Client,
    skill_card_id: Uuid,
    verdict: &str,
    evaluator_source: &str,
    safe_to_apply: bool,
    quality_ok: bool,
    truth_ok: bool,
    utility_delta: f64,
    summary: Option<&str>,
    source_kind: Option<&str>,
    source_event_ids: Option<&Value>,
    artifact_refs: Option<&Value>,
    message_refs: Option<&Value>,
    evidence_span: Option<&Value>,
    derivation_kind: Option<&str>,
    schema_version: Option<&str>,
) -> Result<SkillEvalRecord> {
    let source_event_ids = source_event_ids.cloned().unwrap_or_else(|| json!([]));
    let artifact_refs = artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs = message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = derivation_kind.unwrap_or("extract");
    let schema_version = schema_version.unwrap_or("skill-eval-envelope-v1");
    let skill_scope_row = client
        .query_one(
            r#"
            SELECT skill_scope_type, skill_trust_state, skill_verification_state, skill_shared_promotion_state
            FROM ami.skill_cards
            WHERE skill_card_id = $1
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load skill scope for eval {skill_card_id}"))?;
    let skill_scope_type: String = skill_scope_row.get(0);
    let current_trust_state: String = skill_scope_row.get(1);
    let current_verification_state: String = skill_scope_row.get(2);
    let current_shared_promotion_state: String = skill_scope_row.get(3);
    validate_skill_activity_basis(
        "skill eval",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    if verdict == "candidate_only" && utility_delta.abs() > f64::EPSILON {
        return Err(anyhow!(
            "skill {} cannot change utility for candidate_only verdict",
            skill_card_id
        ));
    }
    if matches!(verdict, "reject" | "quarantine" | "deprecate") && utility_delta > 0.0 {
        return Err(anyhow!(
            "skill {} cannot increase utility for verdict {}",
            skill_card_id,
            verdict
        ));
    }
    let evidence_count: i64 = client
        .query_one(
            "SELECT count(*) FROM ami.skill_evidence_bundles WHERE skill_card_id = $1",
            &[&skill_card_id],
        )
        .await
        .context("failed to count skill evidence bundles")?
        .get(0);
    let matched_trigger_count: i64 = client
        .query_one(
            "SELECT count(*) FROM ami.skill_trigger_matches WHERE skill_card_id = $1 AND matched = true",
            &[&skill_card_id],
        )
        .await
        .context("failed to count matched skill trigger matches")?
        .get(0);
    let shadow_success_count: i64 = client
        .query_one(
            r#"
            SELECT count(*)
            FROM ami.skill_trial_runs
            WHERE skill_card_id = $1
              AND application_mode = 'shadow'
              AND outcome = 'success'
            "#,
            &[&skill_card_id],
        )
        .await
        .context("failed to count shadow trial successes")?
        .get(0);
    let trial_success_count: i64 = client
        .query_one(
            r#"
            SELECT count(*)
            FROM ami.skill_trial_runs
            WHERE skill_card_id = $1
              AND application_mode = 'trial'
              AND outcome = 'success'
            "#,
            &[&skill_card_id],
        )
        .await
        .context("failed to count trial successes")?
        .get(0);
    match verdict {
        "candidate_only" => {}
        "promote_shadow" => {
            if evidence_count == 0 {
                return Err(anyhow!(
                    "skill {} cannot promote to shadow without evidence bundle",
                    skill_card_id
                ));
            }
            if matched_trigger_count == 0 {
                return Err(anyhow!(
                    "skill {} cannot promote to shadow without matched trigger",
                    skill_card_id
                ));
            }
        }
        "promote_trial" => {
            if evidence_count == 0 || shadow_success_count == 0 {
                return Err(anyhow!(
                    "skill {} cannot promote to trial without evidence and successful shadow run",
                    skill_card_id
                ));
            }
            if matched_trigger_count == 0 {
                return Err(anyhow!(
                    "skill {} cannot promote to trial without matched trigger",
                    skill_card_id
                ));
            }
        }
        "promote_verified" => {
            if evidence_count == 0 || trial_success_count == 0 {
                return Err(anyhow!(
                    "skill {} cannot promote to verified without evidence and successful trial run",
                    skill_card_id
                ));
            }
            if matched_trigger_count == 0 {
                return Err(anyhow!(
                    "skill {} cannot promote to verified without matched trigger",
                    skill_card_id
                ));
            }
            if !(safe_to_apply && quality_ok && truth_ok) {
                return Err(anyhow!(
                    "skill {} cannot promote to verified without safe/quality/truth evaluator pass",
                    skill_card_id
                ));
            }
        }
        "approve_shared_promotion" => {
            if skill_scope_type != "project_shared" {
                return Err(anyhow!(
                    "skill {} cannot approve shared promotion outside project_shared scope",
                    skill_card_id
                ));
            }
            if current_trust_state != "verified" || current_verification_state != "verified" {
                return Err(anyhow!(
                    "skill {} cannot approve shared promotion before verified state",
                    skill_card_id
                ));
            }
            if current_shared_promotion_state == "approved" {
                return Err(anyhow!(
                    "skill {} already has approved shared promotion",
                    skill_card_id
                ));
            }
            if !(safe_to_apply && quality_ok && truth_ok) {
                return Err(anyhow!(
                    "skill {} cannot approve shared promotion without safe/quality/truth evaluator pass",
                    skill_card_id
                ));
            }
        }
        "reject" | "quarantine" | "deprecate" => {}
        other => {
            return Err(anyhow!("unsupported skill eval verdict: {other}"));
        }
    }
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.skill_evals(
                skill_card_id,
                verdict,
                evaluator_source,
                safe_to_apply,
                quality_ok,
                truth_ok,
                utility_delta,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb, $11::jsonb, $12::jsonb, $13::jsonb, $14, $15)
            RETURNING
                skill_eval_id,
                skill_card_id,
                verdict,
                safe_to_apply,
                quality_ok,
                truth_ok,
                utility_delta,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            "#,
            &[
                &skill_card_id,
                &verdict,
                &evaluator_source,
                &safe_to_apply,
                &quality_ok,
                &truth_ok,
                &utility_delta,
                &summary,
                &source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &evidence_span,
                &derivation_kind,
                &schema_version,
            ],
        )
        .await
        .with_context(|| format!("failed to record skill eval for {skill_card_id}"))?;
    let (
        next_trust,
        next_verify,
        set_verified_at,
        next_shared_promotion_state,
        next_shared_approved_by,
        next_shared_approval_reason,
        set_shared_approved_at,
    ) = match verdict {
        "candidate_only" => (None, None, false, None, None, None, false),
        "promote_shadow" => (
            Some("shadow"),
            Some("shadow_ready"),
            false,
            None,
            None,
            None,
            false,
        ),
        "promote_trial" => (
            Some("trial"),
            Some("trial_ready"),
            false,
            None,
            None,
            None,
            false,
        ),
        "promote_verified" => (
            Some("verified"),
            Some("verified"),
            true,
            if skill_scope_type == "project_shared" {
                Some("pending_approval")
            } else {
                None
            },
            if skill_scope_type == "project_shared" {
                Some("")
            } else {
                None
            },
            if skill_scope_type == "project_shared" {
                Some("")
            } else {
                None
            },
            false,
        ),
        "approve_shared_promotion" => (
            None,
            None,
            false,
            Some("approved"),
            Some(evaluator_source),
            Some(summary.unwrap_or("shared promotion approved")),
            true,
        ),
        "reject" => (
            Some("candidate"),
            Some("rejected"),
            false,
            None,
            None,
            None,
            false,
        ),
        "quarantine" => (
            Some("quarantined"),
            Some("rejected"),
            false,
            None,
            None,
            None,
            false,
        ),
        "deprecate" => (
            Some("deprecated"),
            Some("verified"),
            false,
            None,
            None,
            None,
            false,
        ),
        _ => (None, None, false, None, None, None, false),
    };
    client
        .execute(
            r#"
            UPDATE ami.skill_cards
            SET skill_trust_state = COALESCE($2, skill_trust_state),
                skill_verification_state = COALESCE($3, skill_verification_state),
                skill_utility_score = skill_utility_score + $4,
                skill_last_verified_at = CASE WHEN $5 THEN now() ELSE skill_last_verified_at END,
                skill_shared_promotion_state = COALESCE($6, skill_shared_promotion_state),
                skill_shared_approved_by = CASE
                    WHEN $6 = 'pending_approval' THEN NULL
                    WHEN $7::text = '' THEN NULL
                    WHEN $7 IS NOT NULL THEN $7
                    ELSE skill_shared_approved_by
                END,
                skill_shared_approval_reason = CASE
                    WHEN $6 = 'pending_approval' THEN NULL
                    WHEN $8::text = '' THEN NULL
                    WHEN $8 IS NOT NULL THEN $8
                    ELSE skill_shared_approval_reason
                END,
                skill_shared_approved_at = CASE
                    WHEN $6 = 'pending_approval' THEN NULL
                    WHEN $9 THEN now()
                    ELSE skill_shared_approved_at
                END,
                updated_at = now()
            WHERE skill_card_id = $1
            "#,
            &[
                &skill_card_id,
                &next_trust,
                &next_verify,
                &utility_delta,
                &set_verified_at,
                &next_shared_promotion_state,
                &next_shared_approved_by,
                &next_shared_approval_reason,
                &set_shared_approved_at,
            ],
        )
        .await
        .context("failed to update skill card after eval")?;
    Ok(SkillEvalRecord {
        skill_eval_id: row.get(0),
        skill_card_id: row.get(1),
        verdict: row.get(2),
        safe_to_apply: row.get(3),
        quality_ok: row.get(4),
        truth_ok: row.get(5),
        utility_delta: row.get(6),
        source_kind: row.get(7),
        source_event_ids: row.get(8),
        artifact_refs: row.get(9),
        message_refs: row.get(10),
        evidence_span: row.get(11),
        derivation_kind: row.get(12),
        schema_version: row.get(13),
    })
}

pub async fn get_skill_eval(client: &Client, skill_eval_id: Uuid) -> Result<SkillEvalRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                skill_eval_id,
                skill_card_id,
                verdict,
                safe_to_apply,
                quality_ok,
                truth_ok,
                utility_delta,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            FROM ami.skill_evals
            WHERE skill_eval_id = $1
            "#,
            &[&skill_eval_id],
        )
        .await
        .with_context(|| format!("failed to load skill eval {}", skill_eval_id))?;
    Ok(SkillEvalRecord {
        skill_eval_id: row.get(0),
        skill_card_id: row.get(1),
        verdict: row.get(2),
        safe_to_apply: row.get(3),
        quality_ok: row.get(4),
        truth_ok: row.get(5),
        utility_delta: row.get(6),
        source_kind: row.get(7),
        source_event_ids: row.get(8),
        artifact_refs: row.get(9),
        message_refs: row.get(10),
        evidence_span: row.get(11),
        derivation_kind: row.get(12),
        schema_version: row.get(13),
    })
}

pub async fn record_skill_reuse_log(
    client: &Client,
    skill_card_id: Uuid,
    reuse_mode: &str,
    task_label: Option<&str>,
    outcome: &str,
    summary: Option<&str>,
    source_event_ids: &[String],
    artifact_refs: &[String],
    source_kind: Option<&str>,
    message_refs: Option<&Value>,
    evidence_span: Option<&Value>,
    derivation_kind: Option<&str>,
    schema_version: Option<&str>,
) -> Result<SkillReuseLogRecord> {
    let reuse_mode = match reuse_mode {
        "shadow" | "trial" | "verified" | "manual_debug" => reuse_mode,
        other => {
            return Err(anyhow!(
                "skill reuse log reuse_mode '{}' must be one of shadow/trial/verified/manual_debug",
                other
            ));
        }
    };
    let source_event_ids = string_array_json(source_event_ids);
    let artifact_refs = string_array_json(artifact_refs);
    let message_refs = message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = derivation_kind.unwrap_or("extract");
    let schema_version = schema_version.unwrap_or("skill-reuse-log-envelope-v1");
    validate_skill_activity_basis(
        "skill reuse log",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    enforce_skill_runtime_model_tool_constraints(
        client,
        skill_card_id,
        &evidence_span,
        None,
        None,
        None,
        "skill reuse log",
    )
    .await?;
    enforce_skill_context_constraints(client, skill_card_id, &evidence_span, "skill reuse log")
        .await?;
    let matched = evidence_span_bool(&evidence_span, "matched");
    let applied = evidence_span_bool(&evidence_span, "applied");
    if reuse_mode == "verified" && !(matched && applied) {
        return Err(anyhow!(
            "skill {} reuse_mode=verified requires matched=true and applied=true",
            skill_card_id
        ));
    }
    if outcome != "neutral" && !(matched && applied) {
        return Err(anyhow!(
            "skill {} reuse outcome requires matched=true and applied=true",
            skill_card_id
        ));
    }
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.skill_reuse_logs(
                skill_card_id,
                reuse_mode,
                task_label,
                outcome,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8::jsonb, $9::jsonb, $10::jsonb, $11, $12)
            RETURNING
                skill_reuse_log_id,
                skill_card_id,
                reuse_mode,
                outcome,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            "#,
            &[
                &skill_card_id,
                &reuse_mode,
                &task_label,
                &outcome,
                &summary,
                &source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &evidence_span,
                &derivation_kind,
                &schema_version,
            ],
        )
        .await
        .with_context(|| format!("failed to record skill reuse log for {skill_card_id}"))?;
    let (success_delta, failure_delta) = match (outcome, matched, applied) {
        ("success", true, true) => (1, 0),
        ("failure", true, true) => (0, 1),
        _ => (0, 0),
    };
    let reuse_delta = i32::from(matched && applied);
    client
        .execute(
            r#"
            UPDATE ami.skill_cards
            SET skill_reuse_count = skill_reuse_count + $4,
                skill_success_count = skill_success_count + $2,
                skill_failure_count = skill_failure_count + $3,
                skill_last_used_at = CASE WHEN $4 > 0 THEN now() ELSE skill_last_used_at END,
                updated_at = now()
            WHERE skill_card_id = $1
            "#,
            &[&skill_card_id, &success_delta, &failure_delta, &reuse_delta],
        )
        .await
        .context("failed to update skill counters after reuse log")?;
    Ok(SkillReuseLogRecord {
        skill_reuse_log_id: row.get(0),
        skill_card_id: row.get(1),
        reuse_mode: row.get(2),
        outcome: row.get(3),
        summary: row.get(4),
        source_kind: row.get(5),
        source_event_ids: row.get(6),
        artifact_refs: row.get(7),
        message_refs: row.get(8),
        evidence_span: row.get(9),
        derivation_kind: row.get(10),
        schema_version: row.get(11),
    })
}

pub async fn get_skill_reuse_log(
    client: &Client,
    skill_reuse_log_id: Uuid,
) -> Result<SkillReuseLogRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                skill_reuse_log_id,
                skill_card_id,
                reuse_mode,
                outcome,
                summary,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            FROM ami.skill_reuse_logs
            WHERE skill_reuse_log_id = $1
            "#,
            &[&skill_reuse_log_id],
        )
        .await
        .with_context(|| format!("failed to load skill reuse log {}", skill_reuse_log_id))?;
    Ok(SkillReuseLogRecord {
        skill_reuse_log_id: row.get(0),
        skill_card_id: row.get(1),
        reuse_mode: row.get(2),
        outcome: row.get(3),
        summary: row.get(4),
        source_kind: row.get(5),
        source_event_ids: row.get(6),
        artifact_refs: row.get(7),
        message_refs: row.get(8),
        evidence_span: row.get(9),
        derivation_kind: row.get(10),
        schema_version: row.get(11),
    })
}

pub async fn list_skill_cards(
    client: &Client,
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    skill_id: Option<&str>,
) -> Result<Vec<SkillCardRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                sc.skill_card_id,
                w.code,
                p.code,
                n.code,
                sc.skill_id,
                sc.skill_version,
                sc.skill_title,
                sc.skill_goal,
                sc.skill_trigger_conditions,
                sc.skill_preconditions,
                sc.skill_execution_steps,
                sc.skill_stop_conditions,
                sc.skill_forbidden_when,
                sc.skill_expected_outcome,
                sc.skill_scope_type,
                sc.skill_owner_scope,
                sc.skill_trust_state,
                sc.skill_verification_state,
                sc.skill_runtime_constraints,
                sc.skill_model_constraints,
                sc.skill_tool_constraints,
                sc.skill_context_constraints,
                sc.skill_source_event_ids,
                sc.skill_artifact_refs,
                sc.skill_evidence_span,
                sc.skill_candidate_class,
                sc.skill_derivation_kind,
                sc.skill_source_kind,
                sc.skill_hot_path_write_eligible,
                sc.skill_background_consolidation_recommended,
                sc.skill_success_count,
                sc.skill_failure_count,
                sc.skill_reuse_count,
                sc.skill_shadow_pass_count,
                sc.skill_shadow_fail_count,
                to_char(sc.skill_last_used_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                to_char(sc.skill_last_verified_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                sc.skill_patch_parent_id,
                sc.skill_merge_group_id,
                sc.skill_shared_promotion_state,
                sc.skill_shared_approved_by,
                sc.skill_shared_approval_reason,
                to_char(sc.skill_shared_approved_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                sc.skill_utility_score
            FROM ami.skill_cards sc
            INNER JOIN ami.workspaces w ON w.workspace_id = sc.workspace_id
            INNER JOIN ami.projects p ON p.project_id = sc.project_id
            INNER JOIN ami.namespaces n ON n.namespace_id = sc.namespace_id
            WHERE ($1::text IS NULL OR p.code = $1)
              AND ($2::text IS NULL OR n.code = $2)
              AND ($3::text IS NULL OR sc.skill_id = $3)
            ORDER BY p.code, n.code, sc.skill_id, sc.skill_version DESC
            "#,
            &[&project_code, &namespace_code, &skill_id],
        )
        .await
        .context("failed to list skill cards")?;
    Ok(rows.iter().map(skill_card_record_from_row).collect())
}

pub async fn build_skill_execution_cards(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    context: Option<&str>,
    runtime: Option<&str>,
    model: Option<&str>,
    tool: Option<&str>,
    allow_trial: bool,
    include_shadow: bool,
    without_amai_but_measuring: bool,
) -> Result<Value> {
    if without_amai_but_measuring {
        return Ok(Value::Array(Vec::new()));
    }
    let mut allowed_states = vec!["verified"];
    if allow_trial {
        allowed_states.push("trial");
    }
    if include_shadow {
        allowed_states.push("shadow");
    }
    let cards = list_skill_cards(client, Some(project_code), Some(namespace_code), None).await?;
    let mut selected: Vec<&SkillCardRecord> = cards
        .iter()
        .filter(|card| allowed_states.contains(&card.skill_trust_state.as_str()))
        .filter(|card| {
            card.skill_scope_type != "project_shared"
                || card.skill_shared_promotion_state == "approved"
        })
        .filter(|card| value_string_array_matches(&card.skill_context_constraints, context))
        .filter(|card| {
            value_string_array_matches_required(&card.skill_runtime_constraints, runtime)
        })
        .filter(|card| value_string_array_matches_required(&card.skill_model_constraints, model))
        .filter(|card| value_string_array_matches_required(&card.skill_tool_constraints, tool))
        .collect();
    selected.sort_by(|left, right| {
        skill_trust_rank(&right.skill_trust_state)
            .cmp(&skill_trust_rank(&left.skill_trust_state))
            .then_with(|| {
                right
                    .skill_utility_score
                    .partial_cmp(&left.skill_utility_score)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| right.skill_reuse_count.cmp(&left.skill_reuse_count))
            .then_with(|| right.skill_success_count.cmp(&left.skill_success_count))
            .then_with(|| left.skill_failure_count.cmp(&right.skill_failure_count))
            .then_with(|| right.skill_version.cmp(&left.skill_version))
            .then_with(|| left.skill_id.cmp(&right.skill_id))
    });
    let payload = Value::Array(
        selected
            .into_iter()
            .map(|card| {
                json!({
                    "skill_card_id": card.skill_card_id,
                    "project": card.project_code,
                    "namespace": card.namespace_code,
                    "skill_id": card.skill_id,
                    "skill_version": card.skill_version,
                    "skill_title": card.skill_title,
                    "skill_goal": card.skill_goal,
                    "skill_trigger_conditions": card.skill_trigger_conditions,
                    "skill_execution_steps": card.skill_execution_steps,
                    "skill_preconditions": card.skill_preconditions,
                    "skill_stop_conditions": card.skill_stop_conditions,
                    "skill_forbidden_when": card.skill_forbidden_when,
                    "skill_expected_outcome": card.skill_expected_outcome,
                    "skill_scope_type": card.skill_scope_type,
                    "skill_owner_scope": card.skill_owner_scope,
                    "skill_trust_state": card.skill_trust_state,
                    "skill_verification_state": card.skill_verification_state,
                    "skill_runtime_constraints": card.skill_runtime_constraints,
                    "skill_model_constraints": card.skill_model_constraints,
                    "skill_tool_constraints": card.skill_tool_constraints,
                    "skill_context_constraints": card.skill_context_constraints,
                    "skill_source_event_ids": card.skill_source_event_ids,
                    "skill_artifact_refs": card.skill_artifact_refs,
                    "skill_evidence_span": card.skill_evidence_span,
                    "skill_candidate_class": card.skill_candidate_class,
                    "skill_derivation_kind": card.skill_derivation_kind,
                    "skill_source_kind": card.skill_source_kind,
                    "skill_hot_path_write_eligible": card.skill_hot_path_write_eligible,
                    "skill_background_consolidation_recommended": card.skill_background_consolidation_recommended,
                    "skill_utility_score": card.skill_utility_score,
                    "skill_success_count": card.skill_success_count,
                    "skill_failure_count": card.skill_failure_count,
                    "skill_reuse_count": card.skill_reuse_count,
                    "skill_shadow_pass_count": card.skill_shadow_pass_count,
                    "skill_shadow_fail_count": card.skill_shadow_fail_count,
                    "skill_last_used_at": card.skill_last_used_at,
                    "skill_last_verified_at": card.skill_last_verified_at
                    ,"skill_patch_parent_id": card.skill_patch_parent_id
                    ,"skill_merge_group_id": card.skill_merge_group_id
                    ,"skill_shared_promotion_state": card.skill_shared_promotion_state
                    ,"skill_shared_approved_by": card.skill_shared_approved_by
                    ,"skill_shared_approval_reason": card.skill_shared_approval_reason
                    ,"skill_shared_approved_at": card.skill_shared_approved_at
                })
            })
            .collect(),
    );
    Ok(payload)
}

fn skill_trust_rank(trust_state: &str) -> i32 {
    match trust_state {
        "verified" => 3,
        "trial" => 2,
        "shadow" => 1,
        _ => 0,
    }
}

fn nested_text_field(value: &Value, outer_key: &str, inner_key: &str) -> Option<String> {
    value
        .get(outer_key)
        .and_then(Value::as_object)
        .and_then(|item| item.get(inner_key))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

async fn build_skill_history_payload(client: &Client, skill_card_id: Uuid) -> Result<Vec<Value>> {
    let scope_row = client
        .query_one(
            r#"
            SELECT namespace_id, skill_id, skill_merge_group_id
            FROM ami.skill_cards
            WHERE skill_card_id = $1
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load skill history scope for {skill_card_id}"))?;
    let namespace_id: Uuid = scope_row.get(0);
    let skill_id: String = scope_row.get(1);
    let merge_group_id: Option<Uuid> = scope_row.get(2);

    let history_rows = client
        .query(
            r#"
            SELECT
                skill_card_id,
                skill_id,
                skill_version,
                skill_title,
                skill_trust_state,
                skill_verification_state,
                skill_patch_parent_id,
                skill_merge_group_id,
                skill_shared_promotion_state,
                skill_shared_approved_by,
                skill_shared_approval_reason,
                to_char(skill_shared_approved_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                skill_source_event_ids,
                skill_artifact_refs,
                skill_evidence_span,
                to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM ami.skill_cards
            WHERE namespace_id = $1
              AND (
                    skill_id = $2
                    OR ($3::uuid IS NOT NULL AND skill_merge_group_id = $3)
                    OR ($3::uuid IS NOT NULL AND skill_card_id = $3)
                  )
            ORDER BY skill_version ASC, created_at ASC, skill_card_id ASC
            "#,
            &[&namespace_id, &skill_id, &merge_group_id],
        )
        .await
        .with_context(|| format!("failed to load skill history for {skill_card_id}"))?;

    Ok(history_rows
        .iter()
        .map(|row| {
            let evidence_span: Value = row.get(14);
            json!({
                "skill_card_id": row.get::<_, Uuid>(0),
                "skill_id": row.get::<_, String>(1),
                "skill_version": row.get::<_, i32>(2),
                "skill_title": row.get::<_, String>(3),
                "skill_trust_state": row.get::<_, String>(4),
                "skill_verification_state": row.get::<_, String>(5),
                "skill_patch_parent_id": row.get::<_, Option<Uuid>>(6),
                "skill_merge_group_id": row.get::<_, Option<Uuid>>(7),
                "skill_shared_promotion_state": row.get::<_, String>(8),
                "skill_shared_approved_by": row.get::<_, Option<String>>(9),
                "skill_shared_approval_reason": row.get::<_, Option<String>>(10),
                "skill_shared_approved_at": row.get::<_, Option<String>>(11),
                "skill_source_event_ids": row.get::<_, Value>(12),
                "skill_artifact_refs": row.get::<_, Value>(13),
                "changed_by": nested_text_field(&evidence_span, "skill_change_summary", "changed_by"),
                "change_reason": nested_text_field(&evidence_span, "skill_change_summary", "change_reason"),
                "refinement_action": evidence_span
                    .get("skill_refinement_decision")
                    .and_then(Value::as_object)
                    .and_then(|item| item.get("action"))
                    .and_then(Value::as_str),
                "created_at": row.get::<_, Option<String>>(15),
                "updated_at": row.get::<_, Option<String>>(16),
            })
        })
        .collect())
}

pub async fn build_skill_review_payload(client: &Client, skill_card_id: Uuid) -> Result<Value> {
    let card_row = client
        .query_opt(
            r#"
            SELECT
                sc.skill_card_id,
                w.code,
                p.code,
                n.code,
                sc.skill_id,
                sc.skill_version,
                sc.skill_title,
                sc.skill_goal,
                sc.skill_trigger_conditions,
                sc.skill_preconditions,
                sc.skill_execution_steps,
                sc.skill_stop_conditions,
                sc.skill_forbidden_when,
                sc.skill_expected_outcome,
                sc.skill_scope_type,
                sc.skill_owner_scope,
                sc.skill_trust_state,
                sc.skill_verification_state,
                sc.skill_runtime_constraints,
                sc.skill_model_constraints,
                sc.skill_tool_constraints,
                sc.skill_context_constraints,
                sc.skill_source_event_ids,
                sc.skill_artifact_refs,
                sc.skill_evidence_span,
                sc.skill_candidate_class,
                sc.skill_derivation_kind,
                sc.skill_source_kind,
                sc.skill_hot_path_write_eligible,
                sc.skill_background_consolidation_recommended,
                sc.skill_success_count,
                sc.skill_failure_count,
                sc.skill_reuse_count,
                sc.skill_shadow_pass_count,
                sc.skill_shadow_fail_count,
                to_char(sc.skill_last_used_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                to_char(sc.skill_last_verified_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                sc.skill_patch_parent_id,
                sc.skill_merge_group_id,
                sc.skill_shared_promotion_state,
                sc.skill_shared_approved_by,
                sc.skill_shared_approval_reason,
                to_char(sc.skill_shared_approved_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                sc.skill_utility_score
            FROM ami.skill_cards sc
            INNER JOIN ami.workspaces w ON w.workspace_id = sc.workspace_id
            INNER JOIN ami.projects p ON p.project_id = sc.project_id
            INNER JOIN ami.namespaces n ON n.namespace_id = sc.namespace_id
            WHERE sc.skill_card_id = $1
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load skill card {skill_card_id} for review"))?;
    let Some(card_row) = card_row else {
        return Err(anyhow!("skill card {skill_card_id} not found"));
    };
    let card = skill_card_record_from_row(&card_row);

    let evidence_rows = client
        .query(
            r#"
            SELECT skill_evidence_bundle_id, evidence_kind, summary, source_event_ids, artifact_refs
            FROM ami.skill_evidence_bundles
            WHERE skill_card_id = $1
            ORDER BY created_at
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load skill evidence bundles for {skill_card_id}"))?;
    let trigger_rows = client
        .query(
            r#"
            SELECT skill_trigger_match_id, match_scope, trigger_input, matched, summary
            FROM ami.skill_trigger_matches
            WHERE skill_card_id = $1
            ORDER BY created_at
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load skill trigger matches for {skill_card_id}"))?;
    let trial_rows = client
        .query(
            r#"
            SELECT skill_trial_run_id, application_mode, task_label, runtime_name, model_name, tool_name, matched, applied, outcome, summary
            FROM ami.skill_trial_runs
            WHERE skill_card_id = $1
            ORDER BY created_at
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load skill trial runs for {skill_card_id}"))?;
    let eval_rows = client
        .query(
            r#"
            SELECT skill_eval_id, verdict, evaluator_source, safe_to_apply, quality_ok, truth_ok, utility_delta, summary
            FROM ami.skill_evals
            WHERE skill_card_id = $1
            ORDER BY created_at
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load skill evals for {skill_card_id}"))?;
    let reuse_rows = client
        .query(
            r#"
            SELECT skill_reuse_log_id, reuse_mode, task_label, outcome, summary, source_event_ids, artifact_refs
            FROM ami.skill_reuse_logs
            WHERE skill_card_id = $1
            ORDER BY created_at
            "#,
            &[&skill_card_id],
        )
        .await
        .with_context(|| format!("failed to load skill reuse logs for {skill_card_id}"))?;
    let history = build_skill_history_payload(client, skill_card_id).await?;

    Ok(json!({
        "skill": {
            "skill_card_id": card.skill_card_id,
            "workspace": card.workspace_code,
            "project": card.project_code,
            "namespace": card.namespace_code,
            "skill_id": card.skill_id,
            "skill_version": card.skill_version,
            "skill_title": card.skill_title,
            "skill_goal": card.skill_goal,
            "skill_trigger_conditions": card.skill_trigger_conditions,
            "skill_preconditions": card.skill_preconditions,
            "skill_execution_steps": card.skill_execution_steps,
            "skill_stop_conditions": card.skill_stop_conditions,
            "skill_forbidden_when": card.skill_forbidden_when,
            "skill_expected_outcome": card.skill_expected_outcome,
            "skill_scope_type": card.skill_scope_type,
            "skill_owner_scope": card.skill_owner_scope,
            "skill_trust_state": card.skill_trust_state,
            "skill_verification_state": card.skill_verification_state,
            "skill_runtime_constraints": card.skill_runtime_constraints,
            "skill_model_constraints": card.skill_model_constraints,
            "skill_tool_constraints": card.skill_tool_constraints,
            "skill_context_constraints": card.skill_context_constraints,
            "skill_source_event_ids": card.skill_source_event_ids,
            "skill_artifact_refs": card.skill_artifact_refs,
            "skill_evidence_span": card.skill_evidence_span,
            "skill_candidate_class": card.skill_candidate_class,
            "skill_derivation_kind": card.skill_derivation_kind,
            "skill_source_kind": card.skill_source_kind,
            "skill_hot_path_write_eligible": card.skill_hot_path_write_eligible,
            "skill_background_consolidation_recommended": card.skill_background_consolidation_recommended,
            "skill_success_count": card.skill_success_count,
            "skill_failure_count": card.skill_failure_count,
            "skill_reuse_count": card.skill_reuse_count,
            "skill_shadow_pass_count": card.skill_shadow_pass_count,
            "skill_shadow_fail_count": card.skill_shadow_fail_count,
            "skill_last_used_at": card.skill_last_used_at,
            "skill_last_verified_at": card.skill_last_verified_at,
            "skill_patch_parent_id": card.skill_patch_parent_id,
            "skill_merge_group_id": card.skill_merge_group_id,
            "skill_shared_promotion_state": card.skill_shared_promotion_state,
            "skill_shared_approved_by": card.skill_shared_approved_by,
            "skill_shared_approval_reason": card.skill_shared_approval_reason,
            "skill_shared_approved_at": card.skill_shared_approved_at,
            "skill_utility_score": card.skill_utility_score
        },
        "evidence_count": evidence_rows.len(),
        "evidence_bundles": evidence_rows.iter().map(|row| {
            json!({
                "skill_evidence_bundle_id": row.get::<_, Uuid>(0),
                "evidence_kind": row.get::<_, String>(1),
                "summary": row.get::<_, Option<String>>(2),
                "source_event_ids": row.get::<_, Value>(3),
                "artifact_refs": row.get::<_, Value>(4),
            })
        }).collect::<Vec<_>>(),
        "trigger_matches": trigger_rows.iter().map(|row| {
            json!({
                "skill_trigger_match_id": row.get::<_, Uuid>(0),
                "match_scope": row.get::<_, String>(1),
                "trigger_input": row.get::<_, String>(2),
                "matched": row.get::<_, bool>(3),
                "summary": row.get::<_, Option<String>>(4),
            })
        }).collect::<Vec<_>>(),
        "trial_runs": trial_rows.iter().map(|row| {
            json!({
                "skill_trial_run_id": row.get::<_, Uuid>(0),
                "application_mode": row.get::<_, String>(1),
                "task_label": row.get::<_, Option<String>>(2),
                "runtime_name": row.get::<_, Option<String>>(3),
                "model_name": row.get::<_, Option<String>>(4),
                "tool_name": row.get::<_, Option<String>>(5),
                "matched": row.get::<_, bool>(6),
                "applied": row.get::<_, bool>(7),
                "outcome": row.get::<_, String>(8),
                "summary": row.get::<_, Option<String>>(9),
            })
        }).collect::<Vec<_>>(),
        "evals": eval_rows.iter().map(|row| {
            json!({
                "skill_eval_id": row.get::<_, Uuid>(0),
                "verdict": row.get::<_, String>(1),
                "evaluator_source": row.get::<_, String>(2),
                "safe_to_apply": row.get::<_, bool>(3),
                "quality_ok": row.get::<_, bool>(4),
                "truth_ok": row.get::<_, bool>(5),
                "utility_delta": row.get::<_, f64>(6),
                "summary": row.get::<_, Option<String>>(7),
            })
        }).collect::<Vec<_>>(),
        "reuse_logs": reuse_rows.iter().map(|row| {
            json!({
                "skill_reuse_log_id": row.get::<_, Uuid>(0),
                "reuse_mode": row.get::<_, String>(1),
                "task_label": row.get::<_, Option<String>>(2),
                "outcome": row.get::<_, String>(3),
                "summary": row.get::<_, Option<String>>(4),
                "source_event_ids": row.get::<_, Value>(5),
                "artifact_refs": row.get::<_, Value>(6),
            })
        }).collect::<Vec<_>>(),
        "history": history,
    }))
}
