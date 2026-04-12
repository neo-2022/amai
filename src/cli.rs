use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

pub const DEFAULT_CLI_CONTINUITY_STARTUP_TOKEN_SOURCE_KIND: &str = "operator_continuity_startup";

#[derive(Debug, Parser)]
#[command(name = "amai")]
#[command(bin_name = "amai")]
#[command(
    about = "Art-memory-agent-index (Amai): Rust-first control plane for multi-project AI-agent continuity and retrieval"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Benchmark {
        #[command(subcommand)]
        command: BenchmarkCommand,
    },
    Continuity {
        #[command(subcommand)]
        command: ContinuityCommand,
    },
    Deployment {
        #[command(subcommand)]
        command: DeploymentCommand,
    },
    Bootstrap {
        #[command(subcommand)]
        command: BootstrapCommand,
    },
    Compat {
        #[command(subcommand)]
        command: CompatCommand,
    },
    Status,
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    Team {
        #[command(subcommand)]
        command: TeamCommand,
    },
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    Role {
        #[command(subcommand)]
        command: RoleCommand,
    },
    AccessPolicy {
        #[command(subcommand)]
        command: AccessPolicyCommand,
    },
    SharedAsset {
        #[command(subcommand)]
        command: SharedAssetCommand,
    },
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    Namespace {
        #[command(subcommand)]
        command: NamespaceCommand,
    },
    Relation {
        #[command(subcommand)]
        command: RelationCommand,
    },
    TransferPolicy {
        #[command(subcommand)]
        command: TransferPolicyCommand,
    },
    ImportPacket {
        #[command(subcommand)]
        command: ImportPacketCommand,
    },
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
    Index {
        #[command(subcommand)]
        command: IndexCommand,
    },
    Verify {
        #[command(subcommand)]
        command: VerifyCommand,
    },
    Observe {
        #[command(subcommand)]
        command: ObserveCommand,
    },
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum BenchmarkCommand {
    List,
    Coverage,
    Explain(BenchmarkArgs),
    ExternalCheck,
    ExternalExplain(BenchmarkArgs),
    ExternalDatasets,
    ExternalDownload(BenchmarkDatasetArgs),
    ExternalPlan(BenchmarkArgs),
    ExternalAdapter(BenchmarkExternalAdapterArgs),
    ExternalHarvest(BenchmarkExternalHarvestArgs),
    ExternalMemoryPrepare(BenchmarkExternalMemoryPrepareArgs),
    ExternalMemoryRun(BenchmarkExternalMemoryRunArgs),
    ExternalMemoryScore(BenchmarkExternalMemoryScoreArgs),
    ExternalMemorySchema(BenchmarkExternalMemorySchemaArgs),
}

#[derive(Debug, Subcommand)]
pub enum DeploymentCommand {
    List,
    Explain(DeploymentTargetArgs),
    Preflight(DeploymentTargetArgs),
}

#[derive(Debug, Subcommand)]
pub enum ContinuityCommand {
    Import(ContinuityImportArgs),
    EnrichThreadIndex(ContinuityThreadIndexEnrichArgs),
    Startup(ContinuityStartupArgs),
    StartupState(ContinuityStartupStateArgs),
    Restore(ContinuityStartupArgs),
    Answer(ContinuityAnswerArgs),
    Handoff(ContinuityHandoffArgs),
    ClientBudgetTarget(ContinuityClientBudgetTargetArgs),
    CompactChat(ContinuityCompactChatArgs),
    RotateChat(ContinuityRotateChatArgs),
}

#[derive(Debug, Subcommand)]
pub enum BootstrapCommand {
    Stack(BootstrapStackArgs),
    Preflight(BootstrapPreflightArgs),
    AgentPreflight(BootstrapAgentPreflightArgs),
    Install(BootstrapOnboardingArgs),
    Onboarding(BootstrapOnboardingArgs),
    Reconnect(BootstrapReconnectArgs),
    Remove(BootstrapDisconnectArgs),
    Disconnect(BootstrapDisconnectArgs),
}

#[derive(Debug, Subcommand)]
pub enum CompatCommand {
    Check,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    Register(ProjectRegisterArgs),
    List(ProjectListArgs),
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceCommand {
    Ensure(WorkspaceEnsureArgs),
    List(WorkspaceListArgs),
}

#[derive(Debug, Subcommand)]
pub enum TeamCommand {
    Ensure(TeamEnsureArgs),
    List(TeamListArgs),
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    Ensure(AgentEnsureArgs),
    List(AgentListArgs),
}

#[derive(Debug, Subcommand)]
pub enum RoleCommand {
    Ensure(RoleEnsureArgs),
    List(RoleListArgs),
}

#[derive(Debug, Subcommand)]
pub enum AccessPolicyCommand {
    Ensure(AccessPolicyEnsureArgs),
    Get(AccessPolicyGetArgs),
    List(AccessPolicyListArgs),
}

#[derive(Debug, Subcommand)]
pub enum SharedAssetCommand {
    Ensure(SharedAssetEnsureArgs),
    Bind(SharedAssetBindArgs),
    Get(SharedAssetGetArgs),
    List(SharedAssetListArgs),
}

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    CreateCandidate(SkillCreateCandidateArgs),
    AddEvidence(SkillAddEvidenceArgs),
    GetEvidence(SkillGetEvidenceArgs),
    RecordTriggerMatch(SkillRecordTriggerMatchArgs),
    GetTriggerMatch(SkillGetTriggerMatchArgs),
    RecordTrialRun(SkillRecordTrialRunArgs),
    GetTrialRun(SkillGetTrialRunArgs),
    RecordEval(SkillRecordEvalArgs),
    GetEval(SkillGetEvalArgs),
    RecordReuse(SkillRecordReuseArgs),
    GetReuse(SkillGetReuseArgs),
    List(SkillListArgs),
    Review(SkillReviewArgs),
    ExecutionCard(SkillExecutionCardArgs),
}

#[derive(Debug, Subcommand)]
pub enum NamespaceCommand {
    Ensure(NamespaceEnsureArgs),
}

#[derive(Debug, Subcommand)]
pub enum RelationCommand {
    Add(RelationAddArgs),
    Update(RelationUpdateArgs),
}

#[derive(Debug, Subcommand)]
pub enum TransferPolicyCommand {
    Ensure(TransferPolicyEnsureArgs),
    Get(TransferPolicyGetArgs),
    List(TransferPolicyListArgs),
}

#[derive(Debug, Subcommand)]
pub enum ImportPacketCommand {
    Create(ImportPacketCreateArgs),
    Get(ImportPacketGetArgs),
    Update(ImportPacketUpdateArgs),
    List(ImportPacketListArgs),
    ReconcileQuarantine(ImportPacketReconcileQuarantineArgs),
}

#[derive(Debug, Subcommand)]
pub enum MemoryCommand {
    CreateItem(MemoryItemCreateArgs),
    GetItem(MemoryItemGetArgs),
    UpdateItem(MemoryItemUpdateArgs),
    CreateProvenance(MemoryProvenanceCreateArgs),
    GetProvenance(MemoryProvenanceGetArgs),
    CreateArtifactRef(ArtifactRefCreateArgs),
    GetArtifactRef(ArtifactRefGetArgs),
    GetLatestRawEvent(MemoryRawEventGetArgs),
    ListWriteOutbox(MemoryWriteOutboxListArgs),
    CreateTaskNode(TaskNodeCreateArgs),
    GetTaskNode(TaskNodeGetArgs),
    CreateTaskEvent(TaskEventCreateArgs),
    GetTaskEvent(TaskEventGetArgs),
    CreateCard(MemoryCardCreateArgs),
    GetCard(MemoryCardGetArgs),
    ListCards(MemoryCardListArgs),
    SupersedeCard(MemoryCardSupersedeArgs),
    UpdateCardTruthState(MemoryCardUpdateTruthStateArgs),
    ApplyCardUpdate(MemoryCardApplyUpdateArgs),
    CreateEdge(MemoryEdgeCreateArgs),
    GetEdge(MemoryEdgeGetArgs),
    CreateConflict(MemoryConflictCreateArgs),
    GetConflict(MemoryConflictGetArgs),
    CreateLinkDecision(MemoryLinkDecisionCreateArgs),
    GetLinkDecision(MemoryLinkDecisionGetArgs),
    CreatePendingLinkProposal(PendingLinkProposalCreateArgs),
    GetPendingLinkProposal(PendingLinkProposalGetArgs),
    CreateRelationEdge(MemoryRelationEdgeCreateArgs),
    GetRelationEdge(MemoryRelationEdgeGetArgs),
    ListRelationEdges(MemoryRelationEdgeListArgs),
    CreateRestorePack(RestorePackCreateArgs),
    GetRestorePack(RestorePackGetArgs),
    CreatePolicyRule(PolicyRuleCreateArgs),
    GetPolicyRule(PolicyRuleGetArgs),
    CreateQuarantineItem(QuarantineItemCreateArgs),
    GetQuarantineItem(QuarantineItemGetArgs),
    CreateRetrievalTrace(RetrievalTraceCreateArgs),
    GetRetrievalTrace(RetrievalTraceGetArgs),
    Consolidate(ConsolidateArgs),
    RunJob(ForgettingJobRunArgs),
    Prune(PruneArgs),
    ArchiveCold(ArchiveColdArgs),
    Revalidate(RevalidateArgs),
    TouchAccess(TouchAccessArgs),
    ExplainForgetting(ExplainForgettingArgs),
}

#[derive(Debug, Args)]
pub struct MemoryItemCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "source-project")]
    pub source_project: Option<String>,
    #[arg(long = "import-packet-id")]
    pub import_packet_id: Option<String>,
    #[arg(long = "owner-agent")]
    pub owner_agent: Option<String>,
    #[arg(long = "item-kind")]
    pub item_kind: String,
    #[arg(long = "identity-key")]
    pub identity_key: Option<String>,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long)]
    pub body: Option<String>,
    #[arg(long = "sensitivity-class")]
    pub sensitivity_class: Option<String>,
    #[arg(long = "truth-state")]
    pub truth_state: Option<String>,
    #[arg(long = "trust-state")]
    pub trust_state: Option<String>,
    #[arg(long = "verification-state")]
    pub verification_state: Option<String>,
    #[arg(long = "lifecycle-state")]
    pub lifecycle_state: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json", default_value = "{}")]
    pub evidence_span_json: String,
    #[arg(long = "derivation-kind")]
    pub derivation_kind: Option<String>,
    #[arg(long = "observed-at-epoch-ms")]
    pub observed_at_epoch_ms: Option<i64>,
    #[arg(long = "recorded-at-epoch-ms")]
    pub recorded_at_epoch_ms: Option<i64>,
    #[arg(long = "valid-from-epoch-ms")]
    pub valid_from_epoch_ms: Option<i64>,
    #[arg(long = "valid-to-epoch-ms")]
    pub valid_to_epoch_ms: Option<i64>,
    #[arg(long = "last-verified-at-epoch-ms")]
    pub last_verified_at_epoch_ms: Option<i64>,
    #[arg(long = "object-version")]
    pub object_version: Option<i64>,
    #[arg(long = "causation-id")]
    pub causation_id: Option<String>,
    #[arg(long = "correlation-id")]
    pub correlation_id: Option<String>,
    #[arg(long = "utility-score")]
    pub utility_score: Option<f64>,
    #[arg(long = "freshness-score")]
    pub freshness_score: Option<f64>,
    #[arg(long = "retention-class")]
    pub retention_class: Option<String>,
    #[arg(long = "ttl-epoch-ms")]
    pub ttl_epoch_ms: Option<i64>,
    #[arg(long = "imported-from-json", default_value = "{}")]
    pub imported_from_json: String,
    #[arg(long = "schema-version")]
    pub schema_version: Option<String>,
    #[arg(long = "superseded-by-memory-item-id")]
    pub superseded_by_memory_item_id: Option<String>,
    #[arg(long = "metadata-json", default_value = "{}")]
    pub metadata_json: String,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct MemoryItemGetArgs {
    #[arg(long = "memory-item-id")]
    pub memory_item_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryItemUpdateArgs {
    #[arg(long = "memory-item-id")]
    pub memory_item_id: String,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "superseded-by-memory-item-id")]
    pub superseded_by_memory_item_id: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct MemoryProvenanceCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "memory-item-id")]
    pub memory_item_id: Option<String>,
    #[arg(long = "source-kind")]
    pub source_kind: String,
    #[arg(long = "source-event-id")]
    pub source_event_id: Option<String>,
    #[arg(long = "source-snapshot-id")]
    pub source_snapshot_id: Option<String>,
    #[arg(long = "artifact-ref-id")]
    pub artifact_ref_id: Option<String>,
    #[arg(long = "trust-level")]
    pub trust_level: Option<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json", default_value = "{}")]
    pub evidence_span_json: String,
    #[arg(long = "derivation-kind")]
    pub derivation_kind: Option<String>,
    #[arg(long = "observed-at-epoch-ms")]
    pub observed_at_epoch_ms: Option<i64>,
    #[arg(long = "recorded-at-epoch-ms")]
    pub recorded_at_epoch_ms: Option<i64>,
    #[arg(long = "valid-from-epoch-ms")]
    pub valid_from_epoch_ms: Option<i64>,
    #[arg(long = "valid-to-epoch-ms")]
    pub valid_to_epoch_ms: Option<i64>,
    #[arg(long = "schema-version")]
    pub schema_version: Option<String>,
    #[arg(long = "details-json", default_value = "{}")]
    pub details_json: String,
}

#[derive(Debug, Args)]
pub struct MemoryProvenanceGetArgs {
    #[arg(long = "memory-provenance-id")]
    pub memory_provenance_id: String,
}

#[derive(Debug, Args)]
pub struct ArtifactRefCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "artifact-kind")]
    pub artifact_kind: String,
    #[arg(long)]
    pub bucket: String,
    #[arg(long = "object-key")]
    pub object_key: String,
    #[arg(long = "content-type")]
    pub content_type: Option<String>,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json", default_value = "{}")]
    pub evidence_span_json: String,
    #[arg(long = "derivation-kind")]
    pub derivation_kind: Option<String>,
    #[arg(long = "schema-version")]
    pub schema_version: Option<String>,
    #[arg(long = "metadata-json", default_value = "{}")]
    pub metadata_json: String,
}

#[derive(Debug, Args)]
pub struct ArtifactRefGetArgs {
    #[arg(long = "artifact-ref-id")]
    pub artifact_ref_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryRawEventGetArgs {
    #[arg(long = "memory-item-id")]
    pub memory_item_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryWriteOutboxListArgs {
    #[arg(long = "memory-item-id")]
    pub memory_item_id: String,
}

#[derive(Debug, Args)]
pub struct TaskNodeCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "parent-task-node-id")]
    pub parent_task_node_id: Option<String>,
    #[arg(long = "memory-item-id")]
    pub memory_item_id: Option<String>,
    #[arg(long = "task-key")]
    pub task_key: Option<String>,
    #[arg(long = "task-role")]
    pub task_role: Option<String>,
    #[arg(long)]
    pub headline: String,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "next-step")]
    pub next_step: Option<String>,
    #[arg(long = "execution-state")]
    pub execution_state: Option<String>,
    #[arg(long = "lifecycle-state")]
    pub lifecycle_state: Option<String>,
    #[arg(long)]
    pub confidence: Option<f64>,
    #[arg(long = "current-score")]
    pub current_score: Option<f64>,
    #[arg(long = "reopened-count")]
    pub reopened_count: Option<i32>,
    #[arg(long = "child-count")]
    pub child_count: Option<i32>,
    #[arg(long = "closed-child-count")]
    pub closed_child_count: Option<i32>,
    #[arg(long = "pending-return-count")]
    pub pending_return_count: Option<i32>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(long = "status-payload-json", default_value = "{}")]
    pub status_payload_json: String,
    #[arg(long = "metadata-json", default_value = "{}")]
    pub metadata_json: String,
    #[arg(long = "opened-at-epoch-ms")]
    pub opened_at_epoch_ms: Option<i64>,
    #[arg(long = "closed-at-epoch-ms")]
    pub closed_at_epoch_ms: Option<i64>,
    #[arg(long = "archived-at-epoch-ms")]
    pub archived_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct TaskNodeGetArgs {
    #[arg(long = "task-node-id")]
    pub task_node_id: String,
}

#[derive(Debug, Args)]
pub struct TaskEventCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "task-node-id")]
    pub task_node_id: String,
    #[arg(long = "source-snapshot-id")]
    pub source_snapshot_id: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_id: Option<String>,
    #[arg(long = "event-kind")]
    pub event_kind: String,
    #[arg(long = "prior-execution-state")]
    pub prior_execution_state: Option<String>,
    #[arg(long = "next-execution-state")]
    pub next_execution_state: Option<String>,
    #[arg(long = "prior-lifecycle-state")]
    pub prior_lifecycle_state: Option<String>,
    #[arg(long = "next-lifecycle-state")]
    pub next_lifecycle_state: Option<String>,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "raw_capture")]
    pub derivation_kind: String,
    #[arg(long = "schema-version", default_value = "task-event-envelope-v1")]
    pub schema_version: String,
    #[arg(long = "event-payload-json", default_value = "{}")]
    pub event_payload_json: String,
    #[arg(long = "recorded-at-epoch-ms")]
    pub recorded_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct TaskEventGetArgs {
    #[arg(long = "task-event-id")]
    pub task_event_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryCardCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub summary: String,
    #[arg(long)]
    pub body: String,
    #[arg(long)]
    pub tag: Vec<String>,
    #[arg(long = "provenance-json", default_value = "{}")]
    pub provenance_json: String,
    #[arg(long = "fact-subject")]
    pub fact_subject: Option<String>,
    #[arg(long = "fact-predicate")]
    pub fact_predicate: Option<String>,
    #[arg(long = "fact-object")]
    pub fact_object: Option<String>,
    #[arg(long = "truth-state")]
    pub truth_state: Option<String>,
    #[arg(long = "verification-state")]
    pub verification_state: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long = "observed-at-epoch-ms")]
    pub observed_at_epoch_ms: Option<i64>,
    #[arg(long = "recorded-at-epoch-ms")]
    pub recorded_at_epoch_ms: Option<i64>,
    #[arg(long = "valid-from-epoch-ms")]
    pub valid_from_epoch_ms: Option<i64>,
    #[arg(long = "valid-to-epoch-ms")]
    pub valid_to_epoch_ms: Option<i64>,
    #[arg(long = "last-verified-at-epoch-ms")]
    pub last_verified_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct MemoryCardGetArgs {
    #[arg(long = "memory-card-id")]
    pub memory_card_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryCardListArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(long = "truth-state")]
    pub truth_state: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
}

#[derive(Debug, Args)]
pub struct MemoryCardSupersedeArgs {
    #[arg(long = "memory-card-id")]
    pub memory_card_id: String,
    #[arg(long = "superseded-by")]
    pub superseded_by: String,
    #[arg(long = "valid-to-epoch-ms")]
    pub valid_to_epoch_ms: Option<i64>,
    #[arg(long = "last-verified-at-epoch-ms")]
    pub last_verified_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct MemoryCardUpdateTruthStateArgs {
    #[arg(long = "memory-card-id")]
    pub memory_card_id: String,
    #[arg(long = "truth-state")]
    pub truth_state: Option<String>,
    #[arg(long = "verification-state")]
    pub verification_state: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long = "last-verified-at-epoch-ms")]
    pub last_verified_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct MemoryCardApplyUpdateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub summary: String,
    #[arg(long)]
    pub body: String,
    #[arg(long)]
    pub tag: Vec<String>,
    #[arg(long = "provenance-json", default_value = "{}")]
    pub provenance_json: String,
    #[arg(long = "fact-subject")]
    pub fact_subject: Option<String>,
    #[arg(long = "fact-predicate")]
    pub fact_predicate: Option<String>,
    #[arg(long = "fact-object")]
    pub fact_object: Option<String>,
    #[arg(long = "truth-state")]
    pub truth_state: Option<String>,
    #[arg(long = "verification-state")]
    pub verification_state: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long = "observed-at-epoch-ms")]
    pub observed_at_epoch_ms: Option<i64>,
    #[arg(long = "recorded-at-epoch-ms")]
    pub recorded_at_epoch_ms: Option<i64>,
    #[arg(long = "valid-from-epoch-ms")]
    pub valid_from_epoch_ms: Option<i64>,
    #[arg(long = "valid-to-epoch-ms")]
    pub valid_to_epoch_ms: Option<i64>,
    #[arg(long = "last-verified-at-epoch-ms")]
    pub last_verified_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    Pack(ContextPackArgs),
    GetPack(ContextPackGetArgs),
    Warm(WarmupCacheArgs),
}

#[derive(Debug, Subcommand)]
pub enum IndexCommand {
    Project(IndexProjectArgs),
}

#[derive(Debug, Subcommand)]
pub enum VerifyCommand {
    Benchmark(Box<VerifyBenchmarkArgs>),
    ColdPath(Box<VerifyColdPathArgs>),
    TokenBenchmark(Box<VerifyTokenBenchmarkArgs>),
    TokenBenchmarkSuite(Box<VerifyTokenBenchmarkSuiteArgs>),
    ProceduralBenchmark(Box<VerifyProceduralBenchmarkArgs>),
    TextCompare(Box<VerifyTextCompareArgs>),
    McpMatrix(Box<VerifyMcpMatrixArgs>),
    MemoryMatrix(Box<VerifyMemoryMatrixArgs>),
    Continuity(Box<VerifyContinuityArgs>),
    Accuracy(VerifyAccuracyArgs),
    Degradation(VerifyDegradationArgs),
    Load(Box<VerifyLoadArgs>),
    Hostile(VerifyHostileArgs),
    Mcp(Box<VerifyMcpArgs>),
}

#[derive(Debug, Subcommand)]
pub enum ObserveCommand {
    Snapshot,
    GetSnapshot(ObserveGetSnapshotArgs),
    ListSnapshots(ObserveListSnapshotsArgs),
    SnapshotPreview,
    #[command(hide = true)]
    BudgetSnapshotPreview,
    SlaCheck,
    Guardrails,
    MaterializeContextPackArtifacts(ObserveMaterializeContextPackArtifactsArgs),
    ListPendingContextPackArtifacts(ObserveListPendingContextPackArtifactsArgs),
    ClientBudgetGate(ObserveClientBudgetGuardArgs),
    ClientBudgetGuard(ObserveClientBudgetGuardArgs),
    ClientBudgetRootCause(ObserveClientBudgetRootCauseArgs),
    #[command(visible_alias = "ctl-launch")]
    ClientBudgetHostControlLaunch(ObserveClientBudgetHostControlLaunchArgs),
    ClientLimitHourlyBurn(ObserveClientLimitHourlyBurnArgs),
    ClientLimitTrendAnalysis(ObserveClientLimitTrendAnalysisArgs),
    TokenReport(ObserveTokenReportArgs),
    TokenEvidencePack(ObserveTokenEvidencePackArgs),
    TokenContractualSources(ObserveTokenContractualSourcesArgs),
    TokenStatementExport(ObserveTokenStatementExportArgs),
    TokenAdjustmentRegistry(ObserveTokenAdjustmentRegistryArgs),
    TokenAdjustmentAdd(ObserveTokenAdjustmentAddArgs),
    TokenWholeCycleAttach(ObserveTokenWholeCycleAttachArgs),
    TokenWholeCycleTurnAttach(ObserveTokenWholeCycleTurnAttachArgs),
    TokenRolloutAssistantGeneration(ObserveTokenRolloutAssistantGenerationArgs),
    CleanupSnapshots(ObserveCleanupSnapshotsArgs),
    CleanupArtifacts(ObserveCleanupArtifactsArgs),
    RepairTokenLedger(ObserveRepairTokenLedgerArgs),
    ReverifyTokenLedger(ObserveReverifyTokenLedgerArgs),
    RelayMemoryWriteOutbox(ObserveRelayMemoryWriteOutboxArgs),
    Serve(ObserveServeArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ObserveClientBudgetGuardArgs {
    #[arg(long, default_value_t = false)]
    pub enforce_reply_gate: bool,
    #[arg(long)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveGetSnapshotArgs {
    #[arg(long = "snapshot-id")]
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveListSnapshotsArgs {
    #[arg(long = "kind")]
    pub kind: String,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(long)]
    pub limit: Option<i64>,
    #[arg(long, default_value_t = false)]
    pub ids_only: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveClientBudgetRootCauseArgs {
    #[arg(long, default_value_t = false)]
    pub enforce_reply_gate: bool,
    #[arg(long)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveClientBudgetHostControlLaunchArgs {
    #[arg(long)]
    pub thread_id: String,
    #[arg(long, default_value_t = false, conflicts_with = "command_id")]
    pub compact_window: bool,
    #[arg(long)]
    pub command_id: Option<String>,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveClientLimitHourlyBurnArgs {
    #[arg(long, default_value_t = 60)]
    pub window_minutes: u64,
    #[arg(long, default_value_t = 10)]
    pub max_live_age_seconds: u64,
    #[arg(long, default_value_t = 55)]
    pub min_history_span_minutes: u64,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveClientLimitTrendAnalysisArgs {
    #[arg(long, default_value_t = 15)]
    pub lookback_minutes: u64,
    #[arg(long, default_value_t = 300)]
    pub window_minutes: u64,
    #[arg(long, default_value_t = 10)]
    pub max_live_age_seconds: u64,
    #[arg(long, default_value_t = false)]
    pub persist_snapshot: bool,
}

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    Serve,
    Config(McpConfigArgs),
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkArgs {
    #[arg(long)]
    pub benchmark: String,
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkDatasetArgs {
    #[arg(long)]
    pub dataset: Option<String>,
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkExternalAdapterArgs {
    #[arg(long)]
    pub benchmark: String,
    #[arg(long)]
    pub dataset: String,
    #[arg(long, default_value_t = false)]
    pub download_missing: bool,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkExternalHarvestArgs {
    #[arg(long)]
    pub benchmark: String,
    #[arg(long)]
    pub dataset: String,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkExternalMemoryPrepareArgs {
    #[arg(long)]
    pub benchmark: String,
    #[arg(long)]
    pub dataset: String,
    #[arg(long, default_value_t = false)]
    pub download_missing: bool,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkExternalMemoryScoreArgs {
    #[arg(long)]
    pub cases: PathBuf,
    #[arg(long)]
    pub predictions: PathBuf,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkExternalMemoryRunArgs {
    #[arg(long)]
    pub requests: PathBuf,
    #[arg(long)]
    pub predictions: PathBuf,
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "bench_runtime")]
    pub namespace: String,
    #[arg(long)]
    pub status: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkExternalMemorySchemaArgs {
    #[arg(long)]
    pub dataset: String,
    #[arg(long)]
    pub benchmark: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct DeploymentTargetArgs {
    #[arg(long)]
    pub target: String,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityImportArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long)]
    pub repo_root: PathBuf,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub bootstrap_file: PathBuf,
    #[arg(long)]
    pub thread_index_file: Option<PathBuf>,
    #[arg(long)]
    pub active_workline_file: Option<PathBuf>,
    #[arg(long)]
    pub memory_dir: Option<PathBuf>,
    #[arg(long)]
    pub transcript_limit: Option<usize>,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityThreadIndexEnrichArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityStartupStateArgs {
    #[arg(long)]
    pub repo_root: PathBuf,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityStartupArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long, default_value_t = false)]
    pub json: bool,
    #[arg(
        long,
        default_value_t = false,
        help = "Emit compact startup runtime-state JSON after materializing startup instead of the full startup payload."
    )]
    pub runtime_state_json: bool,
    #[arg(
        long,
        default_value = DEFAULT_CLI_CONTINUITY_STARTUP_TOKEN_SOURCE_KIND,
        help = "Token ledger source kind for continuity-startup observed whole-cycle events. Plain CLI startup is operator-safe by default; pass live_continuity_startup only for real chat-start flows."
    )]
    pub token_source_kind: String,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityAnswerArgs {
    #[command(flatten)]
    pub startup: ContinuityStartupArgs,
    #[arg(long)]
    pub question: Option<String>,
    #[arg(long, default_value = "last_chat")]
    pub intent: String,
    #[arg(
        long,
        default_value_t = false,
        alias = "include-previous-chat-messages"
    )]
    pub include_chat_messages: bool,
    #[arg(long, default_value_t = 2)]
    pub messages_count: usize,
    #[arg(long)]
    pub chat_reference: Option<String>,
    #[arg(long)]
    pub at_time_rfc3339: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityHandoffArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub headline: String,
    #[arg(long = "next-step")]
    pub next_step: String,
    #[arg(long)]
    pub details_file: Option<PathBuf>,
    #[arg(long = "resolved-headline", action = clap::ArgAction::Append)]
    pub resolved_headlines: Vec<String>,
    #[arg(long = "resolved-task-id", action = clap::ArgAction::Append)]
    pub resolved_task_ids: Vec<String>,
    #[arg(long, default_value_t = false)]
    pub resolve_current_goal: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityRotateChatArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub headline: Option<String>,
    #[arg(long = "next-step")]
    pub next_step: Option<String>,
    #[arg(long)]
    pub details_file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityClientBudgetTargetArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub percent: u64,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityCompactChatArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub headline: Option<String>,
    #[arg(long = "next-step")]
    pub next_step: Option<String>,
    #[arg(long)]
    pub details_file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub launch_host: bool,
    #[arg(long, default_value_t = false)]
    pub runtime_fallback: bool,
    #[arg(long, default_value_t = false)]
    pub skip_handoff: bool,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ProjectRegisterArgs {
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long)]
    pub repo_root: PathBuf,
    #[arg(long)]
    pub default_branch: Option<String>,
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long, default_value = "project_shared")]
    pub visibility_scope: String,
}

#[derive(Debug, Args)]
pub struct ProjectListArgs {
    #[arg(long)]
    pub code: Option<String>,
    #[arg(long = "repo-root")]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct WorkspaceEnsureArgs {
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long, default_value = "active")]
    pub status: String,
}

#[derive(Debug, Args)]
pub struct WorkspaceListArgs {
    #[arg(long)]
    pub code: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TeamEnsureArgs {
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long, default_value = "active")]
    pub status: String,
}

#[derive(Debug, Args)]
pub struct TeamListArgs {
    #[arg(long)]
    pub workspace: Option<String>,
    #[arg(long)]
    pub code: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct AgentEnsureArgs {
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long)]
    pub team: Option<String>,
    #[arg(long)]
    pub role: Option<String>,
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long, default_value = "agent_private")]
    pub visibility_scope: String,
    #[arg(long, default_value = "active")]
    pub status: String,
}

#[derive(Debug, Args)]
pub struct AgentListArgs {
    #[arg(long)]
    pub workspace: Option<String>,
    #[arg(long)]
    pub code: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RoleEnsureArgs {
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long, default_value = "active")]
    pub status: String,
}

#[derive(Debug, Args)]
pub struct RoleListArgs {
    #[arg(long)]
    pub workspace: Option<String>,
    #[arg(long)]
    pub code: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct AccessPolicyEnsureArgs {
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long)]
    pub role: Option<String>,
    #[arg(long)]
    pub team: Option<String>,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long)]
    pub object_class: String,
    #[arg(long, default_value = "project_shared")]
    pub scope_type: String,
    #[arg(long, default_value_t = 100)]
    pub precedence: i32,
    #[arg(long, default_value_t = false)]
    pub can_read: bool,
    #[arg(long, default_value_t = false)]
    pub can_write: bool,
    #[arg(long, default_value_t = false)]
    pub can_link: bool,
    #[arg(long, default_value_t = false)]
    pub can_import: bool,
    #[arg(long, default_value_t = false)]
    pub can_promote: bool,
    #[arg(long, default_value_t = false)]
    pub can_share_further: bool,
    #[arg(long, default_value_t = false)]
    pub can_archive: bool,
    #[arg(long, default_value_t = false)]
    pub can_delete: bool,
    #[arg(long, default_value_t = false)]
    pub can_quarantine: bool,
    #[arg(long, default_value_t = false)]
    pub can_approve_transfer: bool,
    #[arg(long, default_value_t = false)]
    pub human_override: bool,
    #[arg(long)]
    pub override_reason: Option<String>,
    #[arg(long, default_value = "active")]
    pub status: String,
}

#[derive(Debug, Args)]
pub struct AccessPolicyListArgs {
    #[arg(long)]
    pub workspace: Option<String>,
    #[arg(long)]
    pub code: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SharedAssetEnsureArgs {
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long)]
    pub asset_kind: String,
    #[arg(long)]
    pub source_project: Option<String>,
    #[arg(long)]
    pub transfer_policy: Option<String>,
    #[arg(long, default_value = "cross_project_linked")]
    pub visibility_scope: String,
    #[arg(long, default_value = "active")]
    pub status: String,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(long = "schema-version", default_value = "shared-asset-envelope-v1")]
    pub schema_version: String,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TransferPolicyGetArgs {
    #[arg(long = "transfer-policy-id")]
    pub transfer_policy_id: String,
}

#[derive(Debug, Args)]
pub struct TransferPolicyListArgs {
    #[arg(long)]
    pub workspace: Option<String>,
    #[arg(long)]
    pub code: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SharedAssetBindArgs {
    #[arg(long)]
    pub asset: String,
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "consumer")]
    pub binding_kind: String,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(
        long = "schema-version",
        default_value = "shared-asset-project-binding-v1"
    )]
    pub schema_version: String,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SharedAssetListArgs {
    #[arg(long)]
    pub workspace: Option<String>,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub code: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SharedAssetGetArgs {
    #[arg(long = "shared-asset-id")]
    pub shared_asset_id: String,
}

#[derive(Debug, Args)]
pub struct SkillCreateCandidateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "skill-id")]
    pub skill_id: String,
    #[arg(long, default_value_t = 1)]
    pub skill_version: i32,
    #[arg(long = "title")]
    pub skill_title: String,
    #[arg(long = "goal")]
    pub skill_goal: String,
    #[arg(long = "trigger-condition")]
    pub skill_trigger_conditions: Vec<String>,
    #[arg(long = "precondition")]
    pub skill_preconditions: Vec<String>,
    #[arg(long = "execution-step")]
    pub skill_execution_steps: Vec<String>,
    #[arg(long = "stop-condition")]
    pub skill_stop_conditions: Vec<String>,
    #[arg(long = "forbidden-when")]
    pub skill_forbidden_when: Vec<String>,
    #[arg(long = "expected-outcome")]
    pub skill_expected_outcome: Option<String>,
    #[arg(long = "scope-type", default_value = "project_private")]
    pub skill_scope_type: String,
    #[arg(long = "owner-scope", default_value = "project")]
    pub skill_owner_scope: String,
    #[arg(long = "runtime-constraint")]
    pub skill_runtime_constraints: Vec<String>,
    #[arg(long = "model-constraint")]
    pub skill_model_constraints: Vec<String>,
    #[arg(long = "tool-constraint")]
    pub skill_tool_constraints: Vec<String>,
    #[arg(long = "context-constraint")]
    pub skill_context_constraints: Vec<String>,
    #[arg(long = "source-event-id")]
    pub skill_source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub skill_artifact_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub skill_evidence_span_json: Option<String>,
    #[arg(long = "candidate-class")]
    pub skill_candidate_class: Option<String>,
    #[arg(long = "refinement-action")]
    pub skill_refinement_action: Option<String>,
    #[arg(long = "patch-parent-skill-card-id")]
    pub skill_patch_parent_skill_card_id: Option<String>,
    #[arg(long = "merge-group-id")]
    pub skill_merge_group_id: Option<String>,
    #[arg(long = "changed-by")]
    pub skill_changed_by: Option<String>,
    #[arg(long = "change-reason")]
    pub skill_change_reason: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub skill_derivation_kind: String,
}

#[derive(Debug, Args)]
pub struct AccessPolicyGetArgs {
    #[arg(long = "access-policy-id")]
    pub access_policy_id: String,
}

#[derive(Debug, Args)]
pub struct SkillAddEvidenceArgs {
    #[arg(long = "skill-card-id")]
    pub skill_card_id: String,
    #[arg(long = "evidence-kind", default_value = "episode_success")]
    pub evidence_kind: String,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(
        long = "schema-version",
        default_value = "skill-evidence-bundle-envelope-v1"
    )]
    pub schema_version: String,
}

#[derive(Debug, Args)]
pub struct SkillGetEvidenceArgs {
    #[arg(long = "skill-evidence-bundle-id")]
    pub skill_evidence_bundle_id: String,
}

#[derive(Debug, Args)]
pub struct SkillRecordTriggerMatchArgs {
    #[arg(long = "skill-card-id")]
    pub skill_card_id: String,
    #[arg(long = "match-scope", default_value = "project_task")]
    pub match_scope: String,
    #[arg(long = "trigger-input")]
    pub trigger_input: String,
    #[arg(long, default_value_t = false)]
    pub matched: bool,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(
        long = "schema-version",
        default_value = "skill-trigger-match-envelope-v1"
    )]
    pub schema_version: String,
}

#[derive(Debug, Args)]
pub struct SkillGetTriggerMatchArgs {
    #[arg(long = "skill-trigger-match-id")]
    pub skill_trigger_match_id: String,
}

#[derive(Debug, Args)]
pub struct SkillRecordTrialRunArgs {
    #[arg(long = "skill-card-id")]
    pub skill_card_id: String,
    #[arg(long = "application-mode", default_value = "shadow")]
    pub application_mode: String,
    #[arg(long = "task-label")]
    pub task_label: Option<String>,
    #[arg(long)]
    pub context: Option<String>,
    #[arg(long)]
    pub runtime: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub tool: Option<String>,
    #[arg(long, default_value_t = false)]
    pub matched: bool,
    #[arg(long, default_value_t = false)]
    pub applied: bool,
    #[arg(long, default_value = "neutral")]
    pub outcome: String,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(long = "schema-version", default_value = "skill-trial-run-envelope-v1")]
    pub schema_version: String,
}

#[derive(Debug, Args)]
pub struct SkillGetTrialRunArgs {
    #[arg(long = "skill-trial-run-id")]
    pub skill_trial_run_id: String,
}

#[derive(Debug, Args)]
pub struct SkillRecordEvalArgs {
    #[arg(long = "skill-card-id")]
    pub skill_card_id: String,
    #[arg(long)]
    pub verdict: String,
    #[arg(long = "evaluator-source", default_value = "manual_review")]
    pub evaluator_source: String,
    #[arg(long, default_value_t = false)]
    pub safe_to_apply: bool,
    #[arg(long, default_value_t = false)]
    pub quality_ok: bool,
    #[arg(long, default_value_t = false)]
    pub truth_ok: bool,
    #[arg(long, default_value_t = 0.0)]
    pub utility_delta: f64,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(long = "schema-version", default_value = "skill-eval-envelope-v1")]
    pub schema_version: String,
}

#[derive(Debug, Args)]
pub struct SkillGetEvalArgs {
    #[arg(long = "skill-eval-id")]
    pub skill_eval_id: String,
}

#[derive(Debug, Args)]
pub struct SkillRecordReuseArgs {
    #[arg(long = "skill-card-id")]
    pub skill_card_id: String,
    #[arg(long = "reuse-mode", default_value = "shadow")]
    pub reuse_mode: String,
    #[arg(long = "task-label")]
    pub task_label: Option<String>,
    #[arg(long)]
    pub context: Option<String>,
    #[arg(long, default_value_t = false)]
    pub matched: bool,
    #[arg(long, default_value_t = false)]
    pub applied: bool,
    #[arg(long, default_value = "neutral")]
    pub outcome: String,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(long = "schema-version", default_value = "skill-reuse-log-envelope-v1")]
    pub schema_version: String,
}

#[derive(Debug, Args)]
pub struct SkillGetReuseArgs {
    #[arg(long = "skill-reuse-log-id")]
    pub skill_reuse_log_id: String,
}

#[derive(Debug, Args)]
pub struct SkillListArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(long = "skill-id")]
    pub skill_id: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SkillReviewArgs {
    #[arg(long = "skill-card-id")]
    pub skill_card_id: String,
}

#[derive(Debug, Args)]
pub struct SkillExecutionCardArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub context: Option<String>,
    #[arg(long)]
    pub runtime: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub tool: Option<String>,
    #[arg(long, default_value_t = false)]
    pub allow_trial: bool,
    #[arg(long, default_value_t = false)]
    pub include_shadow: bool,
    #[arg(long, default_value_t = false)]
    pub without_amai_but_measuring: bool,
}

#[derive(Debug, Args)]
pub struct NamespaceEnsureArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: Option<String>,
    #[arg(long, default_value = "local_strict")]
    pub retrieval_mode: String,
}

#[derive(Debug, Args)]
pub struct RelationAddArgs {
    #[arg(long)]
    pub source: String,
    #[arg(long)]
    pub target: String,
    #[arg(long)]
    pub relation_type: String,
    #[arg(long)]
    pub project_link_type: Option<String>,
    #[arg(long)]
    pub shared_contour: String,
    #[arg(long, default_value = "cross_project_linked")]
    pub visibility_scope: String,
    #[arg(long, default_value = "active")]
    pub relation_status: String,
    #[arg(long, default_value_t = false)]
    pub requires_approval: bool,
    #[arg(long)]
    pub transfer_policy: Option<String>,
    #[arg(long, default_value = "local_plus_related")]
    pub access_mode: String,
}

#[derive(Debug, Args)]
pub struct RelationUpdateArgs {
    #[arg(long)]
    pub source: String,
    #[arg(long)]
    pub target: String,
    #[arg(long)]
    pub relation_type: String,
    #[arg(long)]
    pub shared_contour: String,
    #[arg(long)]
    pub project_link_type: Option<String>,
    #[arg(long)]
    pub visibility_scope: Option<String>,
    #[arg(long)]
    pub relation_status: Option<String>,
    #[arg(long)]
    pub requires_approval: Option<bool>,
    #[arg(long)]
    pub transfer_policy: Option<String>,
    #[arg(long)]
    pub access_mode: Option<String>,
    #[arg(long)]
    pub actor_agent: Option<String>,
    #[arg(long)]
    pub override_reason: Option<String>,
}

#[derive(Debug, Args)]
pub struct TransferPolicyEnsureArgs {
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long, default_value = "default_deny")]
    pub default_decision: String,
    #[arg(long, default_value_t = false)]
    pub allow_cross_project_read: bool,
    #[arg(long, default_value_t = false)]
    pub allow_import: bool,
    #[arg(long, default_value_t = false)]
    pub allow_verified_writeback: bool,
    #[arg(long, default_value_t = true)]
    pub requires_human_approval: bool,
}

#[derive(Debug, Args)]
pub struct ImportPacketCreateArgs {
    #[arg(long = "source-project")]
    pub source_project: String,
    #[arg(long = "target-project")]
    pub target_project: String,
    #[arg(long)]
    pub transfer_policy: Option<String>,
    #[arg(long)]
    pub requested_by_agent: Option<String>,
    #[arg(long, default_value = "borrowed_unverified")]
    pub status: String,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
    #[arg(long, default_value = "imported")]
    pub imported_by_agent_scope: String,
    #[arg(long, default_value = "proposed")]
    pub trust_state: String,
    #[arg(long, default_value = "unverified")]
    pub verification_state: String,
    #[arg(long, default_value = "borrowed")]
    pub borrowed_status: String,
    #[arg(long, default_value_t = false)]
    pub can_promote_after_verification: bool,
    #[arg(long = "memory-object-id")]
    pub memory_object_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(long = "schema-version", default_value = "import-packet-envelope-v1")]
    pub schema_version: String,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ImportPacketUpdateArgs {
    #[arg(long)]
    pub import_packet_id: String,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
    #[arg(long)]
    pub imported_by_agent_scope: Option<String>,
    #[arg(long)]
    pub trust_state: Option<String>,
    #[arg(long)]
    pub verification_state: Option<String>,
    #[arg(long)]
    pub borrowed_status: Option<String>,
    #[arg(long)]
    pub can_promote_after_verification: Option<bool>,
    #[arg(long)]
    pub actor_agent: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ImportPacketListArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long = "import-packet-id")]
    pub import_packet_id: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ImportPacketReconcileQuarantineArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ImportPacketGetArgs {
    #[arg(long = "import-packet-id")]
    pub import_packet_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryEdgeCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "source-memory-item-id")]
    pub source_memory_item_id: String,
    #[arg(long = "target-memory-item-id")]
    pub target_memory_item_id: String,
    #[arg(long = "edge-kind")]
    pub edge_kind: String,
    #[arg(long = "edge-state", default_value = "active")]
    pub edge_state: String,
    #[arg(long = "trust-state")]
    pub trust_state: Option<String>,
    #[arg(long = "validity-basis")]
    pub validity_basis: Option<String>,
    #[arg(long)]
    pub score: Option<f64>,
    #[arg(long = "evidence-json", default_value = "{}")]
    pub evidence_json: String,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(long = "schema-version", default_value = "memory-edge-envelope-v1")]
    pub schema_version: String,
    #[arg(long = "valid-from-epoch-ms")]
    pub valid_from_epoch_ms: Option<i64>,
    #[arg(long = "valid-to-epoch-ms")]
    pub valid_to_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct MemoryEdgeGetArgs {
    #[arg(long = "memory-edge-id")]
    pub memory_edge_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryConflictCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "left-memory-item-id")]
    pub left_memory_item_id: Option<String>,
    #[arg(long = "right-memory-item-id")]
    pub right_memory_item_id: Option<String>,
    #[arg(long = "conflict-kind")]
    pub conflict_kind: String,
    #[arg(long = "conflict-state", default_value = "open")]
    pub conflict_state: String,
    #[arg(long, default_value = "medium")]
    pub severity: String,
    #[arg(long)]
    pub summary: String,
    #[arg(long = "evidence-json", default_value = "{}")]
    pub evidence_json: String,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(long = "schema-version", default_value = "memory-conflict-envelope-v1")]
    pub schema_version: String,
    #[arg(long = "resolution-json")]
    pub resolution_json: Option<String>,
    #[arg(long = "detected-at-epoch-ms")]
    pub detected_at_epoch_ms: Option<i64>,
    #[arg(long = "resolved-at-epoch-ms")]
    pub resolved_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct MemoryConflictGetArgs {
    #[arg(long = "memory-conflict-id")]
    pub memory_conflict_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryLinkDecisionCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "task-node-id")]
    pub task_node_id: Option<String>,
    #[arg(long = "retrieval-trace-id")]
    pub retrieval_trace_id: Option<String>,
    #[arg(long = "candidate-task-node-id")]
    pub candidate_task_node_id: Option<String>,
    #[arg(long = "decision-outcome")]
    pub decision_outcome: String,
    #[arg(long, default_value_t = false)]
    pub legality_passed: bool,
    #[arg(long, default_value_t = false)]
    pub scope_filter_passed: bool,
    #[arg(long, default_value_t = false)]
    pub evidence_sufficient: bool,
    #[arg(long = "classifier-label")]
    pub classifier_label: Option<String>,
    #[arg(long = "classifier-score")]
    pub classifier_score: Option<f64>,
    #[arg(long = "decision-reason")]
    pub decision_reason: Option<String>,
    #[arg(long = "decision-payload-json", default_value = "{}")]
    pub decision_payload_json: String,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(
        long = "schema-version",
        default_value = "memory-link-decision-envelope-v1"
    )]
    pub schema_version: String,
    #[arg(long = "recorded-at-epoch-ms")]
    pub recorded_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct MemoryLinkDecisionGetArgs {
    #[arg(long = "memory-link-decision-id")]
    pub memory_link_decision_id: String,
}

#[derive(Debug, Args)]
pub struct PendingLinkProposalCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "task-node-id")]
    pub task_node_id: Option<String>,
    #[arg(long = "retrieval-trace-id")]
    pub retrieval_trace_id: Option<String>,
    #[arg(long = "candidate-task-node-id")]
    pub candidate_task_node_id: Option<String>,
    #[arg(long = "proposal-state", default_value = "pending")]
    pub proposal_state: String,
    #[arg(long = "proposal-reason")]
    pub proposal_reason: String,
    #[arg(long = "evidence-request")]
    pub evidence_request: Option<String>,
    #[arg(long = "evidence-payload-json", default_value = "{}")]
    pub evidence_payload_json: String,
    #[arg(long = "classifier-score")]
    pub classifier_score: Option<f64>,
    #[arg(long = "ttl-epoch-ms")]
    pub ttl_epoch_ms: Option<i64>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(
        long = "schema-version",
        default_value = "pending-link-proposal-envelope-v1"
    )]
    pub schema_version: String,
}

#[derive(Debug, Args)]
pub struct PendingLinkProposalGetArgs {
    #[arg(long = "pending-link-proposal-id")]
    pub pending_link_proposal_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryRelationEdgeCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "source-memory-card-id")]
    pub source_memory_card_id: String,
    #[arg(long = "target-memory-card-id")]
    pub target_memory_card_id: String,
    #[arg(long = "relation-type")]
    pub relation_type: String,
    #[arg(long = "relation-state", default_value = "active")]
    pub relation_state: String,
    #[arg(long = "evidence-json", default_value = "{}")]
    pub evidence_json: String,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json")]
    pub evidence_span_json: Option<String>,
    #[arg(long = "derivation-kind", default_value = "extract")]
    pub derivation_kind: String,
    #[arg(
        long = "schema-version",
        default_value = "memory-relation-edge-envelope-v1"
    )]
    pub schema_version: String,
    #[arg(long = "recorded-at-epoch-ms")]
    pub recorded_at_epoch_ms: Option<i64>,
    #[arg(long = "valid-from-epoch-ms")]
    pub valid_from_epoch_ms: Option<i64>,
    #[arg(long = "valid-to-epoch-ms")]
    pub valid_to_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct MemoryRelationEdgeGetArgs {
    #[arg(long = "memory-relation-edge-id")]
    pub memory_relation_edge_id: String,
}

#[derive(Debug, Args)]
pub struct MemoryRelationEdgeListArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "memory-card-id")]
    pub memory_card_ids: Vec<String>,
    #[arg(long = "at-epoch-ms")]
    pub at_epoch_ms: Option<i64>,
    #[arg(long, default_value_t = 64)]
    pub limit: i64,
}

#[derive(Debug, Args)]
pub struct RestorePackCreateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long = "agent-scope")]
    pub agent_scope: Option<String>,
    #[arg(long = "session-id")]
    pub session_id: Option<String>,
    #[arg(long = "thread-id")]
    pub thread_id: Option<String>,
    #[arg(long = "source-snapshot-id")]
    pub source_snapshot_id: Option<String>,
    #[arg(long = "pack-kind")]
    pub pack_kind: String,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json", default_value = "{}")]
    pub evidence_span_json: String,
    #[arg(long = "derivation-kind")]
    pub derivation_kind: Option<String>,
    #[arg(long = "schema-version")]
    pub schema_version: Option<String>,
    #[arg(long)]
    pub headline: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "payload-json")]
    pub payload_json: String,
    #[arg(long = "captured-at-epoch-ms")]
    pub captured_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct RestorePackGetArgs {
    #[arg(long = "restore-pack-id")]
    pub restore_pack_id: String,
}

#[derive(Debug, Args)]
pub struct PolicyRuleCreateArgs {
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(long = "rule-code")]
    pub rule_code: String,
    #[arg(long = "rule-scope")]
    pub rule_scope: String,
    #[arg(long = "rule-kind")]
    pub rule_kind: String,
    #[arg(long = "rule-status")]
    pub rule_status: Option<String>,
    #[arg(long)]
    pub precedence: Option<i32>,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json", default_value = "{}")]
    pub evidence_span_json: String,
    #[arg(long = "derivation-kind")]
    pub derivation_kind: Option<String>,
    #[arg(long = "schema-version")]
    pub schema_version: Option<String>,
    #[arg(long = "rule-payload-json")]
    pub rule_payload_json: String,
}

#[derive(Debug, Args)]
pub struct PolicyRuleGetArgs {
    #[arg(long = "policy-rule-id")]
    pub policy_rule_id: String,
}

#[derive(Debug, Args)]
pub struct QuarantineItemCreateArgs {
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(long = "entity-kind")]
    pub entity_kind: String,
    #[arg(long = "entity-id")]
    pub entity_id: Option<String>,
    #[arg(long = "quarantine-reason")]
    pub quarantine_reason: String,
    #[arg(long = "quarantine-state")]
    pub quarantine_state: Option<String>,
    #[arg(long = "evidence-json")]
    pub evidence_json: String,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json", default_value = "{}")]
    pub evidence_span_json: String,
    #[arg(long = "derivation-kind")]
    pub derivation_kind: Option<String>,
    #[arg(long = "schema-version")]
    pub schema_version: Option<String>,
    #[arg(long = "quarantined-at-epoch-ms")]
    pub quarantined_at_epoch_ms: Option<i64>,
    #[arg(long = "released-at-epoch-ms")]
    pub released_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Args)]
pub struct QuarantineItemGetArgs {
    #[arg(long = "quarantine-item-id")]
    pub quarantine_item_id: String,
}

#[derive(Debug, Args)]
pub struct RetrievalTraceCreateArgs {
    #[arg(long, default_value = "default")]
    pub workspace: String,
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub namespace: String,
    #[arg(long = "context-pack-id")]
    pub context_pack_id: Option<String>,
    #[arg(long = "query-text")]
    pub query_text: String,
    #[arg(long = "requested-mode")]
    pub requested_mode: Option<String>,
    #[arg(long = "effective-mode")]
    pub effective_mode: Option<String>,
    #[arg(long = "scope-filter-json")]
    pub scope_filter_json: String,
    #[arg(long = "candidate-summary-json")]
    pub candidate_summary_json: String,
    #[arg(long = "rerank-summary-json")]
    pub rerank_summary_json: String,
    #[arg(long = "evidence-sufficiency-json")]
    pub evidence_sufficiency_json: String,
    #[arg(long = "source-kind")]
    pub source_kind: Option<String>,
    #[arg(long = "source-event-id")]
    pub source_event_ids: Vec<String>,
    #[arg(long = "artifact-ref")]
    pub artifact_refs: Vec<String>,
    #[arg(long = "message-ref")]
    pub message_refs: Vec<String>,
    #[arg(long = "evidence-span-json", default_value = "{}")]
    pub evidence_span_json: String,
    #[arg(long = "derivation-kind")]
    pub derivation_kind: Option<String>,
    #[arg(long = "schema-version")]
    pub schema_version: Option<String>,
    #[arg(long = "final-decision")]
    pub final_decision: String,
    #[arg(long = "temporal-query-epoch-ms")]
    pub temporal_query_epoch_ms: Option<i64>,
    #[arg(long = "trace-payload-json")]
    pub trace_payload_json: String,
}

#[derive(Debug, Args)]
pub struct RetrievalTraceGetArgs {
    #[arg(long = "retrieval-trace-id")]
    pub retrieval_trace_id: String,
}

#[derive(Debug, Args)]
pub struct ConsolidateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub namespace: String,
    #[arg(long = "now-epoch-ms")]
    pub now_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ForgettingJobKind {
    #[value(name = "de_duplication_job")]
    DeDuplication,
    #[value(name = "summarization_job")]
    Summarization,
    #[value(name = "compaction_job")]
    Compaction,
    #[value(name = "pruning_job")]
    Pruning,
    #[value(name = "cold_archive_job")]
    ColdArchive,
    #[value(name = "revalidation_job")]
    Revalidation,
}

#[derive(Debug, Args)]
pub struct ForgettingJobRunArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub namespace: String,
    #[arg(long = "job-kind", value_enum)]
    pub job_kind: ForgettingJobKind,
    #[arg(long = "now-epoch-ms")]
    pub now_epoch_ms: Option<i64>,
    #[arg(long = "utility-threshold", default_value = "0.05")]
    pub utility_threshold: f64,
    #[arg(long = "freshness-threshold", default_value = "0.05")]
    pub freshness_threshold: f64,
    #[arg(long = "stale-days", default_value = "30")]
    pub stale_days: i64,
}

#[derive(Debug, Args)]
pub struct PruneArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub namespace: String,
    #[arg(long = "now-epoch-ms")]
    pub now_epoch_ms: Option<i64>,
    #[arg(long = "utility-threshold", default_value = "0.05")]
    pub utility_threshold: f64,
}

#[derive(Debug, Args)]
pub struct ArchiveColdArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub namespace: String,
    #[arg(long = "stale-days", default_value = "30")]
    pub stale_days: i64,
}

#[derive(Debug, Args)]
pub struct RevalidateArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub namespace: String,
    #[arg(long = "freshness-threshold", default_value = "0.05")]
    pub freshness_threshold: f64,
}

#[derive(Debug, Args)]
pub struct TouchAccessArgs {
    #[arg(long = "memory-item-id")]
    pub memory_item_id: String,
}

#[derive(Debug, Args)]
pub struct ExplainForgettingArgs {
    #[arg(long = "memory-item-id")]
    pub memory_item_id: String,
}

#[derive(Debug, Clone, Args)]
pub struct ContextPackArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub query: String,
    #[arg(long)]
    pub retrieval_mode: Option<String>,
    #[arg(long, default_value_t = false)]
    pub disable_cache: bool,
    #[arg(long, default_value_t = 5)]
    pub limit_documents: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_symbols: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_chunks: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_semantic_chunks: usize,
    #[arg(
        long,
        help = "Optional epoch-ms timestamp to resolve temporal truth at an exact time."
    )]
    pub at_epoch_ms: Option<i64>,
    #[arg(
        long,
        default_value = "live_context_pack",
        help = "Token ledger source kind for this context-pack call. Use proof_/verify_ prefixes for engineering runs so they do not contaminate live tokenonomics."
    )]
    pub token_source_kind: String,
    #[arg(
        long,
        help = "Optional whole-cycle override for actual client-side prompt tokens in the same meter the upstream client/provider reports."
    )]
    pub client_prompt_tokens: Option<u64>,
    #[arg(
        long,
        help = "Optional whole-cycle observed assistant generation tokens for this context-pack event."
    )]
    pub assistant_generation_tokens: Option<u64>,
    #[arg(
        long,
        help = "Optional whole-cycle observed non-retrieval tool overhead tokens for this context-pack event."
    )]
    pub tool_overhead_tokens: Option<u64>,
    #[arg(
        long,
        help = "Optional whole-cycle observed continuity-restore tokens outside retrieval for this context-pack event."
    )]
    pub continuity_restore_tokens: Option<u64>,
}

#[derive(Debug, Clone, Args)]
pub struct ContextPackGetArgs {
    #[arg(long = "context-pack-id")]
    pub context_pack_id: String,
}

#[derive(Debug, Clone, Args)]
pub struct WarmupCacheArgs {
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub projects: Vec<String>,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long, default_value = "README")]
    pub query: String,
    #[arg(long)]
    pub retrieval_mode: Option<String>,
    #[arg(long, default_value_t = 4)]
    pub limit_documents: usize,
    #[arg(long, default_value_t = 4)]
    pub limit_symbols: usize,
    #[arg(long, default_value_t = 4)]
    pub limit_chunks: usize,
    #[arg(long, default_value_t = 4)]
    pub limit_semantic_chunks: usize,
}

#[derive(Debug, Args)]
pub struct IndexProjectArgs {
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub path: PathBuf,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub limit_files: Option<usize>,
    #[arg(
        long,
        help = "Optional newline-delimited file with exact relative paths to index deterministically"
    )]
    pub paths_file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub skip_embeddings: bool,
    #[arg(
        long,
        default_value_t = false,
        help = "Preserve existing namespace documents; delete only paths absent from the current index set"
    )]
    pub preserve_namespace_documents: bool,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyBenchmarkArgs {
    #[command(flatten)]
    pub context: ContextPackArgs,
    #[arg(long, default_value_t = 1)]
    pub warmup: usize,
    #[arg(long, default_value_t = 5)]
    pub iterations: usize,
    #[arg(long, default_value_t = false)]
    pub persist: bool,
    #[arg(long)]
    pub max_mean_ms: Option<u128>,
    #[arg(long)]
    pub max_p95_ms: Option<u128>,
    #[arg(long)]
    pub max_p99_ms: Option<u128>,
    #[arg(long)]
    pub max_max_ms: Option<u128>,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyColdPathArgs {
    #[arg(
        long,
        default_value = "config/cold_benchmark_manifest.toml",
        help = "Path to the machine-readable cold benchmark dataset manifest"
    )]
    pub manifest: PathBuf,
    #[arg(
        long,
        default_value_t = 2,
        help = "How many full dataset cycles to run for the long-run contour"
    )]
    pub cycles: usize,
    #[arg(
        long,
        default_value_t = 85.0,
        help = "Thermal guard in Celsius. When exceeded, the runner pauses or stops."
    )]
    pub thermal_guard_celsius: f64,
    #[arg(
        long,
        default_value_t = 20,
        help = "How many seconds to cool down before retrying after a thermal spike"
    )]
    pub cooldown_seconds: u64,
    #[arg(
        long,
        default_value_t = 2,
        help = "How many cooldown retries are allowed before the run stops"
    )]
    pub max_cooldown_retries: usize,
    #[arg(
        long,
        default_value_t = 25.0,
        help = "Minimum free disk GiB required to continue the run"
    )]
    pub min_disk_free_gib: f64,
    #[arg(
        long,
        default_value_t = false,
        help = "Reindex every repo on every cycle instead of only before the first cycle"
    )]
    pub reindex_each_cycle: bool,
    #[arg(
        long,
        default_value_t = false,
        help = "Skip indexing and assume the manifest repos are already indexed"
    )]
    pub skip_index: bool,
    #[arg(
        long,
        default_value = "state/cold-benchmark/latest",
        help = "Directory for report, JSON summary and per-sample CSV"
    )]
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyAccuracyArgs {
    #[arg(long, default_value = "project_alpha")]
    pub project: String,
    #[arg(long, default_value = "project_beta")]
    pub related_project: String,
    #[arg(long, default_value = "review")]
    pub namespace: String,
    #[arg(
        long,
        default_value = "config/red_team_retrieval_isolation.toml",
        help = "Path to the machine-readable red-team retrieval isolation suite manifest"
    )]
    pub manifest: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyLoadArgs {
    #[command(flatten)]
    pub context: ContextPackArgs,
    #[arg(long, default_value_t = 8)]
    pub workers: usize,
    #[arg(long, default_value_t = 25)]
    pub iterations_per_worker: usize,
    #[arg(long, default_value_t = 1)]
    pub warmup_per_worker: usize,
    #[arg(long, default_value_t = false)]
    pub persist: bool,
    #[arg(long)]
    pub max_p95_ms: Option<u128>,
    #[arg(long)]
    pub min_qps: Option<f64>,
    #[arg(long)]
    pub max_error_rate: Option<f64>,
    #[arg(long, default_value_t = false)]
    pub record_live_context: bool,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyDegradationArgs {
    #[arg(long, default_value = "all")]
    pub scenario: String,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyTokenBenchmarkArgs {
    #[command(flatten)]
    pub context: ContextPackArgs,
    #[arg(long, default_value = "o200k_base")]
    pub tokenizer: String,
    #[arg(long, default_value_t = 200)]
    pub naive_limit_files: usize,
    #[arg(long, default_value_t = 32768)]
    pub naive_max_bytes_per_file: usize,
    #[arg(long, default_value_t = 3.0)]
    pub min_savings_factor: f64,
    #[arg(long, default_value_t = 50.0)]
    pub min_savings_percent: f64,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyTokenBenchmarkSuiteArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub retrieval_mode: Option<String>,
    #[arg(long, action = clap::ArgAction::Append)]
    pub query: Vec<String>,
    #[arg(long)]
    pub queries_file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub disable_cache: bool,
    #[arg(long, default_value_t = 5)]
    pub limit_documents: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_symbols: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_chunks: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_semantic_chunks: usize,
    #[arg(long, default_value = "o200k_base")]
    pub tokenizer: String,
    #[arg(long, default_value_t = 200)]
    pub naive_limit_files: usize,
    #[arg(long, default_value_t = 32768)]
    pub naive_max_bytes_per_file: usize,
    #[arg(long, default_value_t = 1.2)]
    pub min_mean_savings_factor: f64,
    #[arg(long, default_value_t = 15.0)]
    pub min_mean_savings_percent: f64,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyProceduralBenchmarkArgs {
    #[arg(long = "json-file")]
    pub json_file: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyTextCompareArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub retrieval_mode: Option<String>,
    #[arg(long)]
    pub cases_file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub disable_cache: bool,
    #[arg(long, default_value_t = 5)]
    pub limit_documents: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_symbols: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_chunks: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_semantic_chunks: usize,
    #[arg(long, default_value = "o200k_base")]
    pub tokenizer: String,
    #[arg(long, default_value_t = 200)]
    pub naive_limit_files: usize,
    #[arg(long, default_value_t = 32768)]
    pub naive_max_bytes_per_file: usize,
    #[arg(long, default_value_t = 1.0)]
    pub min_hybrid_hit_ratio: f64,
    #[arg(long, default_value_t = 1.0)]
    pub min_hybrid_head_hit_ratio: f64,
    #[arg(long, default_value_t = 1.2)]
    pub min_hybrid_savings_factor: f64,
}

#[derive(Debug, Args)]
pub struct VerifyHostileArgs {
    #[arg(long, default_value = "all")]
    pub scenario: String,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyMcpArgs {
    #[command(flatten)]
    pub context: ContextPackArgs,
    #[arg(long, default_value = "o200k_base")]
    pub tokenizer: String,
    #[arg(long, default_value_t = 20)]
    pub naive_limit_files: usize,
    #[arg(long, default_value_t = 32768)]
    pub naive_max_bytes_per_file: usize,
    #[arg(long, default_value_t = 1.2)]
    pub min_savings_factor: f64,
    #[arg(long, default_value_t = 15.0)]
    pub min_savings_percent: f64,
    #[arg(long, value_enum, default_value_t = VerifyMcpScope::Full)]
    pub proof_scope: VerifyMcpScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum VerifyMcpScope {
    Full,
    TokenLedger,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyMcpMatrixArgs {
    #[arg(long, default_value = "live_mcpbench_local")]
    pub matrix: String,
    #[arg(long, default_value = "project_alpha")]
    pub project: String,
    #[arg(long, default_value = "project_beta")]
    pub related_project: String,
    #[arg(long, default_value = "review")]
    pub namespace: String,
    #[arg(long, default_value = "codex_5h")]
    pub budget_profile: String,
    #[arg(long)]
    pub min_success_rate: Option<f64>,
    #[arg(long)]
    pub max_p95_ms: Option<f64>,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyMemoryMatrixArgs {
    #[arg(long, default_value = "letta_memory_local")]
    pub matrix: String,
    #[arg(long, default_value = "memory_eval")]
    pub project_prefix: String,
    #[arg(long)]
    pub min_success_rate: Option<f64>,
    #[arg(long)]
    pub min_mean_score: Option<f64>,
    #[arg(long)]
    pub max_p95_ms: Option<f64>,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyContinuityArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveServeArgs {
    #[arg(long, default_value = "0.0.0.0:9464")]
    pub bind: String,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveRelayMemoryWriteOutboxArgs {
    #[arg(long, default_value_t = 64)]
    pub limit: i64,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveListPendingContextPackArtifactsArgs {
    #[arg(long, default_value_t = 64)]
    pub limit: i64,
    #[arg(long)]
    pub context_pack_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenReportArgs {
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenEvidencePackArgs {
    #[arg(long, default_value = "lifetime")]
    pub scope: String,
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenContractualSourcesArgs {
    #[arg(long, default_value = "rolling_window")]
    pub scope: String,
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenStatementExportArgs {
    #[arg(long, default_value = "lifetime")]
    pub scope: String,
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenAdjustmentRegistryArgs {
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenAdjustmentAddArgs {
    #[arg(long)]
    pub scope: String,
    #[arg(long, default_value = "adjustment_entry")]
    pub kind: String,
    #[arg(long, default_value = "requested")]
    pub status: String,
    #[arg(long)]
    pub reason_code: String,
    #[arg(long)]
    pub tokens_delta: Option<i64>,
    #[arg(long)]
    pub amount_delta: Option<f64>,
    #[arg(long)]
    pub currency_profile: Option<String>,
    #[arg(long)]
    pub related_statement_id: Option<String>,
    #[arg(long, default_value_t = false)]
    pub resolve_related_statement_id: bool,
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
    #[arg(long)]
    pub adjustment_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenWholeCycleAttachArgs {
    #[arg(long)]
    pub context_pack_id: String,
    #[arg(long)]
    pub client_prompt_tokens: Option<u64>,
    #[arg(long)]
    pub assistant_generation_tokens: Option<u64>,
    #[arg(long)]
    pub tool_overhead_tokens: Option<u64>,
    #[arg(long)]
    pub continuity_restore_tokens: Option<u64>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenWholeCycleTurnAttachArgs {
    #[arg(long)]
    pub thread_id: String,
    #[arg(long)]
    pub turn_id: String,
    #[arg(long = "context-pack-id", required = true)]
    pub context_pack_ids: Vec<String>,
    #[arg(long)]
    pub assistant_generation_tokens: u64,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenRolloutAssistantGenerationArgs {
    #[arg(long)]
    pub rollout_path: Option<PathBuf>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub apply: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveCleanupSnapshotsArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveCleanupArtifactsArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long, default_value_t = false)]
    pub aggressive: bool,
    #[arg(long)]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveRepairTokenLedgerArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<i64>,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub project_prefix: Option<String>,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(long)]
    pub source_kind: Option<String>,
    #[arg(long)]
    pub correlation_id: Option<String>,
    #[arg(long)]
    pub rewrite_source_kind: Option<String>,
    #[arg(long)]
    pub repair_reason: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveReverifyTokenLedgerArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapStackArgs {
    #[arg(long, default_value = "default")]
    pub stack_profile: String,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapPreflightArgs {
    #[arg(long, default_value = "default")]
    pub stack_profile: String,
}

#[derive(Debug, Clone, Args)]
pub struct McpConfigArgs {
    #[arg(long, default_value = "generic")]
    pub client: String,
    #[arg(long, default_value = "amai")]
    pub server_name: String,
    #[arg(long, default_value = "auto")]
    pub launcher_platform: String,
    #[arg(long)]
    pub ssh_destination: Option<String>,
    #[arg(long)]
    pub remote_repo_root: Option<PathBuf>,
    #[arg(long)]
    pub command: Option<String>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapOnboardingArgs {
    #[arg(long, default_value = "auto")]
    pub client: String,
    #[arg(long, default_value = "default")]
    pub stack_profile: String,
    #[arg(long, default_value_t = false)]
    pub yes: bool,
    #[arg(long, default_value = "auto")]
    pub launcher_platform: String,
    #[arg(long)]
    pub ssh_destination: Option<String>,
    #[arg(long)]
    pub remote_repo_root: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub workspace_root: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub skip_release_build: bool,
    #[arg(long, default_value_t = false)]
    pub skip_stack: bool,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapAgentPreflightArgs {
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub workspace_root: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapDisconnectArgs {
    #[arg(long, default_value = "auto")]
    pub client: String,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long, default_value_t = true)]
    pub purge_empty_file: bool,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapReconnectArgs {
    #[arg(long, default_value = "auto")]
    pub client: String,
    #[arg(long, default_value_t = false)]
    pub yes: bool,
    #[arg(long, default_value = "auto")]
    pub launcher_platform: String,
    #[arg(long)]
    pub ssh_destination: Option<String>,
    #[arg(long)]
    pub remote_repo_root: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub workspace_root: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_register_cli_defaults_to_default_workspace_and_project_scope() {
        let cli = Cli::parse_from([
            "amai",
            "project",
            "register",
            "--code",
            "art",
            "--display-name",
            "Art",
            "--repo-root",
            ".",
        ]);
        let Command::Project { command } = cli.command else {
            panic!("expected project command");
        };
        let ProjectCommand::Register(args) = command else {
            panic!("expected project register command");
        };
        assert_eq!(args.workspace, "default");
        assert_eq!(args.visibility_scope, "project_shared");
    }

    #[test]
    fn relation_add_cli_accepts_control_plane_fields() {
        let cli = Cli::parse_from([
            "amai",
            "relation",
            "add",
            "--source",
            "art",
            "--target",
            "art_docs",
            "--relation-type",
            "depends_on",
            "--project-link-type",
            "shared_codebase",
            "--shared-contour",
            "memory_fabric",
            "--visibility-scope",
            "cross_project_linked",
            "--relation-status",
            "active",
            "--requires-approval",
            "--transfer-policy",
            "default_deny",
        ]);
        let Command::Relation { command } = cli.command else {
            panic!("expected relation command");
        };
        let RelationCommand::Add(args) = command else {
            panic!("expected relation add command");
        };
        assert_eq!(args.project_link_type.as_deref(), Some("shared_codebase"));
        assert_eq!(args.visibility_scope, "cross_project_linked");
        assert_eq!(args.relation_status, "active");
        assert!(args.requires_approval);
        assert_eq!(args.transfer_policy.as_deref(), Some("default_deny"));
    }

    #[test]
    fn agent_ensure_cli_defaults_to_default_workspace_and_private_scope() {
        let cli = Cli::parse_from([
            "amai",
            "agent",
            "ensure",
            "--code",
            "codex",
            "--display-name",
            "Codex",
        ]);
        let Command::Agent { command } = cli.command else {
            panic!("expected agent command");
        };
        let AgentCommand::Ensure(args) = command else {
            panic!("expected agent ensure command");
        };
        assert_eq!(args.workspace, "default");
        assert_eq!(args.visibility_scope, "agent_private");
        assert_eq!(args.status, "active");
    }

    #[test]
    fn access_policy_ensure_cli_accepts_dynamic_rights_and_bindings() {
        let cli = Cli::parse_from([
            "amai",
            "access-policy",
            "ensure",
            "--workspace",
            "default",
            "--role",
            "operator",
            "--team",
            "core",
            "--project",
            "art",
            "--code",
            "project_fact_reader",
            "--display-name",
            "Project Fact Reader",
            "--object-class",
            "fact",
            "--scope-type",
            "project_shared",
            "--precedence",
            "250",
            "--can-read",
            "--can-link",
            "--can-import",
            "--can-approve-transfer",
            "--human-override",
            "--override-reason",
            "manual policy bootstrap",
        ]);
        let Command::AccessPolicy { command } = cli.command else {
            panic!("expected access-policy command");
        };
        let AccessPolicyCommand::Ensure(args) = command else {
            panic!("expected access-policy ensure command");
        };
        assert_eq!(args.role.as_deref(), Some("operator"));
        assert_eq!(args.team.as_deref(), Some("core"));
        assert_eq!(args.project.as_deref(), Some("art"));
        assert_eq!(args.object_class, "fact");
        assert_eq!(args.scope_type, "project_shared");
        assert_eq!(args.precedence, 250);
        assert!(args.can_read);
        assert!(args.can_link);
        assert!(args.can_import);
        assert!(args.can_approve_transfer);
        assert!(args.human_override);
        assert_eq!(
            args.override_reason.as_deref(),
            Some("manual policy bootstrap")
        );
    }

    #[test]
    fn import_packet_create_cli_accepts_stage1_lifecycle_fields() {
        let cli = Cli::parse_from([
            "amai",
            "import-packet",
            "create",
            "--source-project",
            "art",
            "--target-project",
            "art_docs",
            "--transfer-policy",
            "borrow_guard",
            "--status",
            "borrowed_unverified",
            "--summary",
            "stage1 import",
            "--reason",
            "shared codebase import",
            "--imported-by-agent-scope",
            "cross_project_linked",
            "--trust-state",
            "proposed",
            "--verification-state",
            "unverified",
            "--borrowed-status",
            "borrowed",
            "--can-promote-after-verification",
            "--memory-object-id",
            "memory-1",
            "--memory-object-id",
            "memory-2",
            "--artifact-ref",
            "artifact://trace/1",
        ]);
        let Command::ImportPacket { command } = cli.command else {
            panic!("expected import-packet command");
        };
        let ImportPacketCommand::Create(args) = command else {
            panic!("expected import-packet create command");
        };
        assert_eq!(args.reason.as_deref(), Some("shared codebase import"));
        assert_eq!(args.imported_by_agent_scope, "cross_project_linked");
        assert_eq!(args.trust_state, "proposed");
        assert_eq!(args.verification_state, "unverified");
        assert_eq!(args.borrowed_status, "borrowed");
        assert!(args.can_promote_after_verification);
        assert_eq!(args.memory_object_ids, vec!["memory-1", "memory-2"]);
        assert_eq!(args.artifact_refs, vec!["artifact://trace/1"]);
    }

    #[test]
    fn skill_create_candidate_cli_accepts_seed_fields() {
        let cli = Cli::parse_from([
            "amai",
            "skill",
            "create-candidate",
            "--project",
            "amai",
            "--namespace",
            "continuity",
            "--skill-id",
            "continuity_restore",
            "--title",
            "Continuity Restore",
            "--goal",
            "Recover the previous operator task safely",
            "--trigger-condition",
            "startup gate requires resume",
            "--precondition",
            "continuity runtime state is fresh",
            "--execution-step",
            "read startup_next_action",
            "--stop-condition",
            "required return task acknowledged",
            "--forbidden-when",
            "continuity state is stale",
            "--expected-outcome",
            "resume path is restored without drift",
            "--runtime-constraint",
            "codex",
            "--model-constraint",
            "gpt-5",
            "--tool-constraint",
            "exec_command",
            "--context-constraint",
            "continuity",
            "--source-event-id",
            "event-1",
            "--artifact-ref",
            "artifact://continuity/1",
            "--evidence-span-json",
            "{\"path\":\"docs/restore.md\",\"line_start\":1,\"line_end\":3}",
            "--derivation-kind",
            "extract",
        ]);
        let Command::Skill { command } = cli.command else {
            panic!("expected skill command");
        };
        let SkillCommand::CreateCandidate(args) = command else {
            panic!("expected skill create-candidate command");
        };
        assert_eq!(args.project, "amai");
        assert_eq!(args.namespace, "continuity");
        assert_eq!(args.skill_id, "continuity_restore");
        assert_eq!(args.skill_version, 1);
        assert_eq!(args.skill_title, "Continuity Restore");
        assert_eq!(args.skill_runtime_constraints, vec!["codex"]);
        assert_eq!(args.skill_source_event_ids, vec!["event-1"]);
        assert_eq!(args.skill_artifact_refs, vec!["artifact://continuity/1"]);
        assert_eq!(
            args.skill_evidence_span_json.as_deref(),
            Some("{\"path\":\"docs/restore.md\",\"line_start\":1,\"line_end\":3}")
        );
        assert_eq!(args.skill_derivation_kind, "extract");
    }

    #[test]
    fn skill_execution_card_cli_accepts_trial_shadow_and_without_amai_flags() {
        let cli = Cli::parse_from([
            "amai",
            "skill",
            "execution-card",
            "--project",
            "amai",
            "--namespace",
            "continuity",
            "--context",
            "restore",
            "--runtime",
            "codex",
            "--tool",
            "exec_command",
            "--allow-trial",
            "--include-shadow",
            "--without-amai-but-measuring",
        ]);
        let Command::Skill { command } = cli.command else {
            panic!("expected skill command");
        };
        let SkillCommand::ExecutionCard(args) = command else {
            panic!("expected skill execution-card command");
        };
        assert_eq!(args.project, "amai");
        assert_eq!(args.namespace, "continuity");
        assert_eq!(args.context.as_deref(), Some("restore"));
        assert_eq!(args.runtime.as_deref(), Some("codex"));
        assert_eq!(args.tool.as_deref(), Some("exec_command"));
        assert!(args.allow_trial);
        assert!(args.include_shadow);
        assert!(args.without_amai_but_measuring);
    }

    #[test]
    fn skill_review_cli_accepts_skill_card_id() {
        let cli = Cli::parse_from([
            "amai",
            "skill",
            "review",
            "--skill-card-id",
            "11111111-1111-1111-1111-111111111111",
        ]);
        let Command::Skill { command } = cli.command else {
            panic!("expected skill command");
        };
        let SkillCommand::Review(args) = command else {
            panic!("expected skill review command");
        };
        assert_eq!(args.skill_card_id, "11111111-1111-1111-1111-111111111111");
    }

    #[test]
    fn verify_procedural_benchmark_cli_accepts_json_file() {
        let cli = Cli::parse_from([
            "amai",
            "verify",
            "procedural-benchmark",
            "--json-file",
            "state/procedural-benchmark.json",
        ]);
        let Command::Verify { command } = cli.command else {
            panic!("expected verify command");
        };
        let VerifyCommand::ProceduralBenchmark(args) = command else {
            panic!("expected procedural benchmark command");
        };
        assert_eq!(
            args.json_file,
            PathBuf::from("state/procedural-benchmark.json")
        );
    }

    #[test]
    fn continuity_startup_cli_defaults_to_operator_safe_token_source_kind() {
        let cli = Cli::parse_from(["amai", "continuity", "startup", "--project", "art"]);
        let Command::Continuity { command } = cli.command else {
            panic!("expected continuity command");
        };
        let ContinuityCommand::Startup(args) = command else {
            panic!("expected continuity startup command");
        };
        assert!(!args.runtime_state_json);
        assert_eq!(
            args.token_source_kind,
            DEFAULT_CLI_CONTINUITY_STARTUP_TOKEN_SOURCE_KIND
        );
    }

    #[test]
    fn relation_update_cli_accepts_revoke_and_rescope_fields() {
        let cli = Cli::parse_from([
            "amai",
            "relation",
            "update",
            "--source",
            "art",
            "--target",
            "art_docs",
            "--relation-type",
            "depends_on",
            "--shared-contour",
            "memory_fabric",
            "--visibility-scope",
            "quarantine",
            "--relation-status",
            "forbidden",
            "--requires-approval",
            "true",
            "--actor-agent",
            "codex",
            "--override-reason",
            "legal boundary triggered",
        ]);
        let Command::Relation { command } = cli.command else {
            panic!("expected relation command");
        };
        let RelationCommand::Update(args) = command else {
            panic!("expected relation update command");
        };
        assert_eq!(args.visibility_scope.as_deref(), Some("quarantine"));
        assert_eq!(args.relation_status.as_deref(), Some("forbidden"));
        assert_eq!(args.requires_approval, Some(true));
        assert_eq!(args.actor_agent.as_deref(), Some("codex"));
        assert_eq!(
            args.override_reason.as_deref(),
            Some("legal boundary triggered")
        );
    }

    #[test]
    fn continuity_startup_cli_accepts_runtime_state_json_flag() {
        let cli = Cli::parse_from([
            "amai",
            "continuity",
            "startup",
            "--project",
            "art",
            "--runtime-state-json",
        ]);
        let Command::Continuity { command } = cli.command else {
            panic!("expected continuity command");
        };
        let ContinuityCommand::Startup(args) = command else {
            panic!("expected continuity startup command");
        };
        assert!(args.runtime_state_json);
    }

    #[test]
    fn continuity_answer_cli_inherits_operator_safe_startup_default() {
        let cli = Cli::parse_from([
            "amai",
            "continuity",
            "answer",
            "--project",
            "art",
            "--question",
            "на чем остановились",
        ]);
        let Command::Continuity { command } = cli.command else {
            panic!("expected continuity command");
        };
        let ContinuityCommand::Answer(args) = command else {
            panic!("expected continuity answer command");
        };
        assert_eq!(
            args.startup.token_source_kind,
            DEFAULT_CLI_CONTINUITY_STARTUP_TOKEN_SOURCE_KIND
        );
    }

    #[test]
    fn continuity_rotate_chat_cli_parses() {
        let cli = Cli::parse_from([
            "amai",
            "continuity",
            "rotate-chat",
            "--project",
            "art",
            "--json",
        ]);
        let Command::Continuity { command } = cli.command else {
            panic!("expected continuity command");
        };
        let ContinuityCommand::RotateChat(args) = command else {
            panic!("expected continuity rotate-chat command");
        };
        assert_eq!(args.project.as_deref(), Some("art"));
        assert!(args.json);
        assert!(!args.force);
    }

    #[test]
    fn continuity_handoff_cli_parses_resolve_flags() {
        let cli = Cli::parse_from([
            "amai",
            "continuity",
            "handoff",
            "--project",
            "art",
            "--headline",
            "ExecCtl stale pending-return closure semantics materialized",
            "--next-step",
            "Recheck Art startup queue after explicit resolve path.",
            "--resolve-current-goal",
            "--resolved-headline",
            "Amai continuity migration proof",
            "--resolved-headline",
            "Soft rotate recommendation no longer hard-blocks replies",
            "--resolved-task-id",
            "task::event-123",
        ]);
        let Command::Continuity { command } = cli.command else {
            panic!("expected continuity command");
        };
        let ContinuityCommand::Handoff(args) = command else {
            panic!("expected continuity handoff command");
        };
        assert!(args.resolve_current_goal);
        assert_eq!(
            args.resolved_headlines,
            vec![
                "Amai continuity migration proof".to_string(),
                "Soft rotate recommendation no longer hard-blocks replies".to_string()
            ]
        );
        assert_eq!(args.resolved_task_ids, vec!["task::event-123".to_string()]);
    }

    #[test]
    fn bootstrap_reconnect_cli_parses() {
        let cli = Cli::parse_from([
            "amai",
            "bootstrap",
            "reconnect",
            "--client",
            "codex",
            "--yes",
        ]);
        let Command::Bootstrap { command } = cli.command else {
            panic!("expected bootstrap command");
        };
        let BootstrapCommand::Reconnect(args) = command else {
            panic!("expected bootstrap reconnect command");
        };
        assert_eq!(args.client, "codex");
        assert!(args.yes);
    }

    #[test]
    fn observe_client_budget_guard_cli_parses() {
        let cli = Cli::parse_from(["amai", "observe", "client-budget-guard"]);
        let Command::Observe { command } = cli.command else {
            panic!("expected observe command");
        };
        let ObserveCommand::ClientBudgetGuard(args) = command else {
            panic!("expected client-budget-guard command");
        };
        assert!(!args.enforce_reply_gate);
    }

    #[test]
    fn observe_client_budget_guard_enforce_flag_cli_parses() {
        let cli = Cli::parse_from([
            "amai",
            "observe",
            "client-budget-guard",
            "--enforce-reply-gate",
        ]);
        let Command::Observe { command } = cli.command else {
            panic!("expected observe command");
        };
        let ObserveCommand::ClientBudgetGuard(args) = command else {
            panic!("expected client-budget-guard command");
        };
        assert!(args.enforce_reply_gate);
    }

    #[test]
    fn observe_client_budget_gate_cli_parses() {
        let cli = Cli::parse_from(["amai", "observe", "client-budget-gate"]);
        let Command::Observe { command } = cli.command else {
            panic!("expected observe command");
        };
        let ObserveCommand::ClientBudgetGate(args) = command else {
            panic!("expected client-budget-gate command");
        };
        assert!(!args.enforce_reply_gate);
    }

    #[test]
    fn observe_client_budget_root_cause_cli_parses() {
        let cli = Cli::parse_from(["amai", "observe", "client-budget-root-cause"]);
        let Command::Observe { command } = cli.command else {
            panic!("expected observe command");
        };
        let ObserveCommand::ClientBudgetRootCause(args) = command else {
            panic!("expected client-budget-root-cause command");
        };
        assert!(!args.enforce_reply_gate);
    }

    #[test]
    fn observe_client_budget_root_cause_enforce_flag_cli_parses() {
        let cli = Cli::parse_from([
            "amai",
            "observe",
            "client-budget-root-cause",
            "--enforce-reply-gate",
        ]);
        let Command::Observe { command } = cli.command else {
            panic!("expected observe command");
        };
        let ObserveCommand::ClientBudgetRootCause(args) = command else {
            panic!("expected client-budget-root-cause command");
        };
        assert!(args.enforce_reply_gate);
    }

    #[test]
    fn observe_client_budget_host_control_launch_alias_and_compact_window_cli_parse() {
        let cli = Cli::parse_from([
            "amai",
            "observe",
            "ctl-launch",
            "--thread-id",
            "thread-current",
            "--compact-window",
        ]);
        let Command::Observe { command } = cli.command else {
            panic!("expected observe command");
        };
        let ObserveCommand::ClientBudgetHostControlLaunch(args) = command else {
            panic!("expected client-budget-host-control-launch command");
        };
        assert_eq!(args.thread_id, "thread-current");
        assert!(args.compact_window);
        assert!(args.command_id.is_none());
    }

    #[test]
    fn observe_snapshot_preview_cli_parses() {
        let cli = Cli::parse_from(["amai", "observe", "snapshot-preview"]);
        let Command::Observe { command } = cli.command else {
            panic!("expected observe command");
        };
        assert!(matches!(command, ObserveCommand::SnapshotPreview));
    }

    #[test]
    fn observe_budget_snapshot_preview_cli_parses() {
        let cli = Cli::parse_from(["amai", "observe", "budget-snapshot-preview"]);
        let Command::Observe { command } = cli.command else {
            panic!("expected observe command");
        };
        assert!(matches!(command, ObserveCommand::BudgetSnapshotPreview));
    }
}
#[derive(Debug, Clone, Args)]
pub struct ObserveMaterializeContextPackArtifactsArgs {
    #[arg(long, default_value_t = 32)]
    pub limit: i64,
}
