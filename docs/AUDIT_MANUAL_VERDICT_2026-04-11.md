# Manual Verdict For `amai_full_audit_report.md`

Дата ручной сверки: 2026-04-11

Источник checklist: `/home/art/Загрузки/amai_full_audit_report.md`

## Короткий итог

Полный audit-report вручную пройден по текущему состоянию repo root, а не по историческому snapshot.

Главный вывод:
- часть самых жёстких P0 из отчёта уже устарела и не воспроизводится на текущем дереве;
- часть рисков подтверждается полностью и остаётся живой;
- часть подтверждается только как `historical snapshot finding`, а не как текущий live defect;
- во время этой сверки уже устранены несколько реальных self-consistency проблем:
  - восстановлена корректная startup SHA-строка в `AGENTS.md`;
  - выровнены materialized startup artifacts, `./target/debug/amai status` снова показывает `startup_artifacts: ok`;
  - восстановлены executable bits у 8 proof/helper scripts;
  - часть абсолютных repo-root ссылок в entry/docs/config surface уже переведена в repo-local вид.

## Что считается verdict-категориями

- `подтверждено`
  - проблема или сильная сторона воспроизводится на текущем дереве;
- `частично`
  - тезис верен не полностью или только для части слоя;
- `устарело`
  - тезис был верен для исторического snapshot, но не подтверждается сейчас;
- `historical snapshot finding`
  - тезис относится к старому uploaded snapshot/binary и не доказывает текущий live defect сам по себе.

## Что было проверено дополнительно вручную

- `./target/debug/amai status`
- `cargo run --quiet -- bootstrap agent-preflight --json`
- `cargo test --workspace --all-targets --no-run --offline`
- `./target/debug/amai benchmark coverage`
- `cargo fmt --check`
- наличие docs/scripts/proof bundle по фактическому дереву
- `compose.yaml`, `config/nats/server.conf`, `.env.example`
- фактические размеры крупных Rust/shell/SQL модулей
- `config/cold_repo_pool_seed.tsv`, `config/cold_benchmark_manifest.toml`
- `target/debug/amai.d`, `target/release/amai.d`

## Verdict По Пунктам Аудита

### 0. Meta: `Что было проверено`

**Пункт аудита**
- metadata snapshot `229 файлов`, `42 Rust files`, `114 shell scripts`, `11 markdown docs`, `1 SQL bootstrap`, `25 config files`.

**Ручной verdict**
- `historical snapshot finding`

**Evidence**
- текущий `git status --short` показывает сильно изменившееся дерево и существенно более широкий набор docs/scripts, чем в описанном audit snapshot;
- часть файлов, отмеченных в отчёте как отсутствующие, сейчас уже materialized.

**Риск**
- если использовать эти snapshot-counts как текущую истину, можно принять устаревшие выводы за live defects.

**Нужна ли правка сейчас**
- нет.

**Какой exact fix path**
- верифицировать каждый тезис аудита по текущему дереву, а не по историческим totals.

**Какой proof/smoke обязателен после исправления**
- не требуется.

---

### 1. P0: source snapshot не совпадает с binary из-за `src/forgetting.rs`

**Ручной verdict**
- `устарело` для текущего дерева;
- `historical snapshot finding` для старого uploaded binary claim.

**Evidence**
- `src/forgetting.rs` сейчас существует;
- `target/debug/amai.d` и `target/release/amai.d` оба содержат `src/forgetting.rs`;
- `src/main.rs` wiring на `forgetting::*` теперь согласован с деревом.

**Риск**
- текущий live defect не подтверждается;
- historical mismatch всё ещё важен как урок про binary/source attestation.

**Нужна ли правка сейчас**
- нет как live bug;
- да как process-hardening тема.

**Какой exact fix path**
- для process layer: добавить CI/attestation, которая проверяет соответствие source tree и shipped binary artifacts.

**Какой proof/smoke обязателен после исправления**
- CI check, который сравнивает shipped binary dependencies и текущий source tree manifest.

---

### 2. P0: onboarding/docs fail-closed contract не выполняется

**Ручной verdict**
- `устарело`

**Evidence**
- `docs/AGENT_START_HERE.md`, `docs/IMPLEMENTATION_STATUS.md`, `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md`, `docs/IMPLEMENTATION_GATES.md`, `docs/AMAI_TASK_TREE_PLAN.md`, `docs/AMAI_COMPARE_EXPERIMENT_PLAN.md` сейчас присутствуют;
- `cargo run --quiet -- bootstrap agent-preflight --json` проходит.

**Риск**
- исходный claim больше не описывает текущее состояние.

**Нужна ли правка сейчас**
- нет по самому claim;
- да по поддержанию doc/startup self-consistency.

**Какой exact fix path**
- продолжать держать `agent_preflight` и startup artifacts в зелёном состоянии через guard/CI.

**Какой proof/smoke обязателен после исправления**
- `cargo run --quiet -- bootstrap agent-preflight --json`
- `./target/debug/amai status`

---

### 3. P0: contracts/docs ссылаются на отсутствующие helper/proof scripts

**Ручной verdict**
- `частично`

**Evidence**
- `scripts/agent_preflight.sh`, `scripts/proof_cold_benchmark_canonical.sh`, `scripts/proof_procedural_benchmark.sh`, `scripts/proof_procedural_seed.sh`, `scripts/proof_procedural_shadow_review.sh`, `scripts/proof_negative_procedural_memory.sh`, `scripts/proof_shared_promotion_by_approval.sh`, `scripts/proof_skill_refinement_contour.sh`, `scripts/proof_skill_version_history.sh` сейчас существуют;
- следовательно, исходный `missing scripts` block устарел;
- абсолютные path-bound ссылки сведены к минимуму и не считаются operator-truth source;

**Риск**
- missing-scripts часть уже не live;
- portability / relocation drift остаётся live проблемой.

**Нужна ли правка сейчас**
- да.

**Какой exact fix path**
- довести cleanup абсолютных repo-root ссылок в canonical docs и generated artifacts до repo-local или parameterized form;
- отделить допустимые path-specific operator examples от source-of-truth ссылок.

**Какой proof/smoke обязателен после исправления**
- `rg -n '/home/art/agent-memory-index' AGENTS.md README.md docs config`
- `./target/debug/amai status`
- `cargo run --quiet -- bootstrap agent-preflight --json`

---

### 4. P0: repo не воспроизводим офлайн

**Ручной verdict**
- `частично`

**Evidence**
- `cargo test --workspace --all-targets --no-run --offline` на текущей машине проходит;
- `.cargo/` каталог существует, но внутри нет materialized config/registry policy;
- `vendor/` отсутствует;
- значит deterministic offline reproducibility не закреплена как repo contract, даже если локальная машина уже прогрета кэшем.

**Риск**
- зависимость от warm local cargo cache остаётся;
- это слабый supply-chain/reproducibility baseline.

**Нужна ли правка сейчас**
- да.

**Какой exact fix path**
- материализовать reproducible offline strategy:
  - либо `cargo vendor` + `.cargo/config.toml`;
  - либо document/CI-attested internal mirror strategy;
- закрепить это в repo и CI, а не оставлять как свойство конкретной машины.

**Какой proof/smoke обязателен после исправления**
- fresh-environment `cargo test --workspace --all-targets --no-run --offline`
- CI job без доступа к crates.io

---

### 5. P0: benchmark story слабее позиционирования

**Ручной verdict**
- `подтверждено`

**Evidence**
- `./target/debug/amai benchmark coverage` сейчас всё ещё показывает:
  - `20 total`
  - `0 materialized`
  - `3 partial`
  - `12 mapped`
  - `5 future`

**Риск**
- benchmark mapping есть, но leaderboard-grade measured superiority не доказана.

**Нужна ли правка сейчас**
- да.

**Какой exact fix path**
- переходить от registry/explain-layer к measured benchmark bundles по ключевым contours:
  - memory correctness
  - update fidelity
  - long-horizon continuity
  - multi-agent isolation
  - coding-agent tasks

**Какой proof/smoke обязателен после исправления**
- `./target/debug/amai benchmark coverage`
- benchmark-specific proof bundles и dashboard snapshots

---

### 6. P1: maintainability debt и giant files

**Ручной verdict**
- `подтверждено`

**Evidence**
- текущие размеры ещё больше audit snapshot:
  - `src/token_budget.rs` 35637
  - `src/postgres.rs` 35580
  - `src/dashboard.rs` 26739
  - `src/observe.rs` 13718
  - `src/working_state.rs` 11099
  - `src/continuity.rs` 10652
  - `src/mcp.rs` 7697
  - `src/external_benchmark.rs` 6933
  - `src/retrieval.rs` 6521
- `cargo fmt --check` сейчас продолжает падать на drift как минимум в `src/continuity.rs` и `src/observe.rs`.

**Риск**
- bounded-context границы размыты;
- change-safety и reviewability ухудшаются.

**Нужна ли правка сейчас**
- да, но как staged refactor queue, не как emergency blocker.

**Какой exact fix path**
- запланировать decomposition по domain boundaries:
  - `postgres`: schema/bootstrap vs DAL vs policy/write-path vs tests;
  - `dashboard`: render templates vs payload mapping vs handlers;
  - `token_budget`, `observe`, `continuity`: разбивка по contours.

**Какой proof/smoke обязателен после исправления**
- `cargo fmt --check`
- targeted tests по каждому выделенному bounded context
- maintainability gate / status sync guard где применимо

---

### 7. P1: security / ops baseline не production-grade

**Ручной verdict**
- `подтверждено`

**Evidence**
- `compose.yaml` публикует host ports для Postgres, Qdrant, MinIO, NATS, Prometheus, Grafana;
- `config/nats/server.conf` слушает `0.0.0.0:4222` и `0.0.0.0:8222`, при этом нет named users/passwords;
- `.env.example` содержит `change_me`, `minioadmin`, а Prometheus/Grafana задаются через `latest`;
- `src/postgres.rs` содержит:
  - `GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA ami TO {user};`

**Риск**
- baseline годится для local/dev/lab, но не для hardened deployment.

**Нужна ли правка сейчас**
- да.

**Какой exact fix path**
- подготовить hardened default profile:
  - закрыть лишние host-port exposures;
  - ввести auth/TLS story для NATS/MinIO/Postgres;
  - убрать `latest` tags;
  - документировать least-privilege DB role model.

**Какой proof/smoke обязателен после исправления**
- hardened compose bring-up
- auth-required connectivity checks
- deploy/preflight proof

---

### 8. P1: CLI/operator UX слишком developer-first

**Ручной verdict**
- `частично`

**Evidence**
- исходные claim-ы про backtrace-by-default сейчас не воспроизводятся:
  - `./target/debug/amai status` отдаёт компактный structured output;
  - `set -a && . ./.env.example && set +a && ./target/debug/amai status` тоже не дал backtrace;
  - `cargo run --quiet -- bootstrap agent-preflight --json` проходит;
  - `bootstrap preflight` на этой машине показывает последовательный verdict `машина подходит`.
- operator path теперь binary-first через `./scripts/amai_exec.sh`, с подавлением build chatter по умолчанию
  (`AMAI_EXEC_SUPPRESS_BUILD_NOISE=1`).

**Риск**
- исходный тезис частично устарел;
- cargo warning noise остаётся только в developer path (если принудительно идти через `cargo run`).

**Нужна ли правка сейчас**
- нет, operator path уже разведен и подавляет build chatter.

**Какой exact fix path**
- уже материализован:
  - `./scripts/amai_exec.sh` предпочитает release binary;
  - build chatter подавляется, лог сохраняется в `state/logs/`.

**Какой proof/smoke обязателен после исправления**
- smoke for `./scripts/amai_exec.sh status`
- smoke for `./scripts/amai_exec.sh bootstrap preflight`
- smoke for `./scripts/amai_exec.sh bootstrap agent-preflight --json`

---

### 9. P1: документация перегружена и неканонична

**Ручной verdict**
- `частично`

**Evidence**
- длины документов подтверждаются и даже выросли:
  - `README.md` 3076
  - `docs/OPERATIONS.md` 3392
  - `docs/TOKEN_LEDGER.md` 1633
- но `docs/AGENT_START_HERE.md` сейчас уже есть и выполняет роль entry doc.

**Риск**
- missing-entry-doc часть устарела;
- overload/reference sprawl остаются.

**Нужна ли правка сейчас**
- да, как doc hygiene/hardening.

**Какой exact fix path**
- сохранить `AGENT_START_HERE` как canonical short entry;
- дальше ужать повторяющиеся narrative blocks и уменьшить число абсолютных path-bound ссылок.

**Какой proof/smoke обязателен после исправления**
- `cargo run --quiet -- bootstrap agent-preflight --json`
- docs link/path grep

---

### 10. P1: cold benchmark / external corpus не self-contained

**Ручной verdict**
- `подтверждено`

**Evidence**
- `config/cold_repo_pool_seed.tsv` требует `../Art`, `../my_langgraph_agent`, `../agent-RegArt` и большой набор внешних git repos;
- `config/cold_benchmark_manifest.toml` содержит cases на внешние corpus paths.

**Риск**
- reproducible benchmark path зависит от внешнего corpus окружения;
- это снижает доверие к benchmark portability.

**Нужна ли правка сейчас**
- да.

**Какой exact fix path**
- выделить self-contained fixture tier для mandatory cold benchmark;
- external corpus оставить как expanded profile с pinned revisions/checksums.

**Какой proof/smoke обязателен после исправления**
- self-contained cold benchmark run на clean machine
- manifest completeness check

---

### 11. Сильная сторона: ширина замысла и глубина домена

**Ручной verdict**
- `подтверждено`

**Evidence**
- `Cargo.toml` и код действительно покрывают PostgreSQL, Qdrant, S3/MinIO, NATS, Axum, tree-sitter, columnar stack, SQLite edge cache, MCP, benchmark/eval layers.

**Риск**
- риск не в отсутствии ambition, а в difficulty of keeping it coherent.

**Нужна ли правка сейчас**
- нет.

**Какой exact fix path**
- защищать breadth через non-regression discipline, а не урезать scope без доказательств.

**Какой proof/smoke обязателен после исправления**
- не требуется.

---

### 12. Сильная сторона: богатая DB/model plane

**Ручной verdict**
- `подтверждено`

**Evidence**
- `sql/000_bootstrap.sql` остаётся крупным domain-rich schema bootstrap;
- в коде есть explicit contours для memory/provenance/restore/task/eval.

**Риск**
- богатство модели пока упирается в migration discipline.

**Нужна ли правка сейчас**
- нет как отрицательный finding.

**Какой exact fix path**
- сохранить richness, но развести bootstrap и migration evolution.

**Какой proof/smoke обязателен после исправления**
- schema/migration proof bundle.

---

### 13. Сильная сторона: много tests/proof contours

**Ручной verdict**
- `подтверждено`

**Evidence**
- repo содержит большой набор `proof_*.sh`, а shell parse discipline и test density по Rust-модулям действительно высоки.

**Риск**
- количество proof layers не равно benchmark maturity и не спасает от drift.

**Нужна ли правка сейчас**
- нет.

**Какой exact fix path**
- повышать trust не количеством scripts, а measured coverage и CI enforcement.

**Какой proof/smoke обязателен после исправления**
- N/A

---

### 14. Сильная сторона: живой CLI/runtime surface

**Ручной verdict**
- `подтверждено`

**Evidence**
- работают `status`, `benchmark coverage`, `bootstrap agent-preflight`, `bootstrap preflight`.

**Риск**
- live surface есть, но часть quality claims вокруг него всё ещё требует hardening.

**Нужна ли правка сейчас**
- нет.

**Какой exact fix path**
- продолжать усиливать operator path и startup truth.

**Какой proof/smoke обязателен после исправления**
- CLI smoke bundle.

---

### 15. Infra/dependency layer

**Ручной verdict**
- `подтверждено`

**Evidence**
- `Cargo.toml` и compose/config layer действительно отражают широкий platform stack.

**Риск**
- dependency breadth повышает supply-chain и maintainability нагрузку.

**Нужна ли правка сейчас**
- нет как отдельный defect.

**Какой exact fix path**
- учитывать breadth в reproducibility, security и decomposition queues.

**Какой proof/smoke обязателен после исправления**
- CI for dependency pinning and offline reproducibility.

---

### 16. Файлы, требующие приоритетного рефакторинга

**Ручной verdict**
- `подтверждено` как разумный remediation shortlist.

**Evidence**
- размеры и смешение ответственностей подтверждаются фактическими line counts и содержанием модулей.

**Риск**
- без staged decomposition эти зоны будут продолжать притягивать drift.

**Нужна ли правка сейчас**
- да, но как Queue 3.

**Какой exact fix path**
- refactor roadmap для:
  - `src/main.rs`
  - `src/postgres.rs`
  - `src/dashboard.rs`
  - `src/token_budget.rs`
  - `src/onboarding.rs`
  - `sql/000_bootstrap.sql`

**Какой proof/smoke обязателен после исправления**
- targeted tests + maintainability gate + non-regression proof bundle

---

### 17. Snapshot contradictions section

**Ручной verdict**
- `частично`

**Evidence**
- устарели:
  - `src/main.rs` ↔ missing `src/forgetting.rs`
  - `src/onboarding.rs` ↔ missing onboarding docs
  - `src/onboarding.rs` / `AGENTS.md` ↔ missing `scripts/agent_preflight.sh`
  - `config/benchmark_matrix.toml` ↔ missing procedural proof bundle
- частично живо:
  - абсолютные `/home/art/agent-memory-index` ссылки очищены из docs/contracts/examples.

**Риск**
- старый contradiction set нельзя переносить на current tree без ручной сверки.

**Нужна ли правка сейчас**
- да только для remaining absolute-path drift.

**Какой exact fix path**
- убрать remaining absolute links из canonical docs и generated artifacts;
- historical contradiction claims пометить как closed.

**Какой proof/smoke обязателен после исправления**
- `rg -n '/home/art/agent-memory-index' AGENTS.md README.md docs config`

---

### 18. Shell layer

**Ручной verdict**
- `частично`

**Evidence**
- strong side подтверждена: shell syntax discipline хорошая;
- non-executable list из аудита для 8 scripts был live и был исправлен в этой сверке;
- oversized shell scripts по line count подтверждаются.

**Риск**
- executable-bit issue уже закрыт;
- large scripts остаются maintainability risk.

**Нужна ли правка сейчас**
- частично:
  - executable bit: уже исправлено;
  - decomposition: да, но позже.

**Какой exact fix path**
- держать chmod/runner sanity check в CI;
- постепенно разрезать top-heavy shell scripts по bounded-purpose chunks.

**Какой proof/smoke обязателен после исправления**
- `bash -n` bundle
- executable sanity check over `scripts/proof_*.sh`

---

### 19. SQL layer

**Ручной verdict**
- `подтверждено`

**Evidence**
- `sql/000_bootstrap.sql` остаётся single giant bootstrap/history/drift-repair file на 3963 строки.

**Риск**
- migration evolution и downgrade/recovery остаются недостаточно формализованными.

**Нужна ли правка сейчас**
- да, но как Queue 3 structural refactor.

**Какой exact fix path**
- перейти к numbered migrations + schema journal + drift checker.

**Какой proof/smoke обязателен после исправления**
- migration apply/reapply proof
- upgrade/downgrade smoke

---

### 20. Phases 1-3 remediation из аудита

**Ручной verdict**
- `частично`

**Evidence**
- направления аудита в целом валидны, но стартовые пункты нужно скорректировать:
  - `src/forgetting.rs` уже не missing;
  - onboarding docs и scripts уже materialized;
  - абсолютные ссылки и reproducibility story остаются;
  - startup/self-consistency defect был live локально и был исправлен во время этой сверки.

**Риск**
- если реализовывать старый remediation plan без ручной сверки, можно тратить время на уже закрытые historical defects.

**Нужна ли правка сейчас**
- да, но как updated queue ordering, а не как blind adoption of old phase list.

**Какой exact fix path**
- использовать очереди ниже, а не оригинальные фазы из audit как literal checklist.

**Какой proof/smoke обязателен после исправления**
- consolidated audit recheck

---

### 21. Итоговые оценки по направлениям

**Ручной verdict**
- `частично`

**Evidence**
- directionally still true:
  - benchmark maturity слабее ambition;
  - maintainability и ops/security всё ещё слабые;
  - ambition и domain modeling сильные.
- устарело/смягчено:
  - `Onboarding/source-of-truth integrity = 1/10` больше не соответствует текущему состоянию после восстановления docs/scripts и startup artifacts;
  - reproducibility остаётся проблемой, но не в той форме, как описано исходным P0.

**Риск**
- старые numeric scores уже нельзя принимать как current truth без обновления.

**Нужна ли правка сейчас**
- нет как отдельный defect.

**Какой exact fix path**
- после закрытия Queue 1 и Queue 2 обновить scoring sheet по fresh evidence.

**Какой proof/smoke обязателен после исправления**
- rerun this manual verdict process.

## Additional Live Finding From This Manual Check

### Startup artifacts drift

Во время ручной сверки был обнаружен и устранён live defect, которого нет как отдельного пункта в исходном audit-report:
- `./target/debug/amai status` показывал `startup_artifacts: startup_contract_drift`;
- root cause: расходились `AGENTS.md` и materialized startup artifacts в `.amai/onboarding/...`;
- по итогам исправления `./target/debug/amai status` теперь показывает:
  - `startup_artifacts: ok`
  - `startup_runtime_state: ok`

Это стоит считать фактическим Queue 1 self-consistency fix, выполненным в рамках данной проверки.

## Remediation Plan

### Queue 1: immediate blockers

- Довести reproducibility contract до deterministic offline story, а не rely-on-cache.
- Дочистить remaining absolute repo-root ссылки из canonical docs/config/generated surfaces.
- Добавить CI checks на:
  - startup/doc/script drift
  - broken/missing path refs
  - executable bits for runnable proof scripts
  - `cargo fmt --check`
  - offline no-run build path

Обновление по состоянию remediation:
- canonical doc portability drift уже снят;
- startup/doc/script drift и `cargo fmt --check` уже заведены в machine-readable hygiene contour;
- repo-local Rust dependency layer теперь materialized через `vendor/` и `.cargo/config.toml`;
- native ONNX Runtime artifact для `ort-sys` тоже materialized repo-local через `third_party/onnxruntime/.../libonnxruntime.a`;
- proof `./scripts/proof_offline_no_run_build.sh` на пустом `CARGO_HOME` и с `--offline --locked` теперь проходит.

### Queue 2: near-term hardening

- Hardened ops/security profile:
  - auth/TLS/least privilege
  - убрать `latest`
  - минимизировать host-port exposure
- Operator UX cleanup:
  - binary-first paths
  - suppress cargo/build noise for operator flows
- Self-contained cold benchmark fixture tier.
- Doc portability cleanup across remaining docs.

Обновление по состоянию remediation:
- default compose exposure уже ужесточён до loopback-only published ports через `AMI_STACK_BIND_HOST=127.0.0.1`;
- monitoring defaults больше не используют floating `latest`;
- machine-readable proof `./scripts/proof_ops_security_defaults.sh` materialized и заведён в `repo_hygiene_guard`;
- materialized repo-local cold benchmark fixture tier:
  - `config/cold_benchmark_self_contained.toml`
  - `./scripts/proof_cold_benchmark_self_contained.sh`
  - contour работает только по `Amai` repo, живой прогон проходит и помечается как `proof`, поэтому не перетирает canonical dashboard snapshot;
- materialized optional NATS auth story:
  - committed template `config/nats/server.conf.tpl`
  - rendered runtime config `tmp/nats/server.conf`
  - `./scripts/render_nats_config.sh`
  - `./scripts/proof_nats_auth_render.sh`
  - default local/dev mode остаётся `disabled`, но password-auth contour теперь есть как runnable proof, а не только как audit wish;
- app DB role теперь ужесточён до read-only contract:
  - `ensure_app_role()` больше не выдаёт `INSERT/UPDATE/DELETE` на весь schema `ami`;
  - `./scripts/proof_app_db_role_read_only.sh` подтверждает `SELECT ok / INSERT denied`;
- materialized security hardening contract:
  - `AMI_SECURITY_PROFILE` как явный switch;
  - `./scripts/proof_security_hardening_contract.sh` проверяет TLS/auth требования в hardened mode;
- materialized MinIO/Postgres auth/TLS deployment contract:
  - render-layer: `./scripts/render_postgres_config.sh`;
  - templates: `config/postgres/postgresql.conf.tpl`, `config/postgres/pg_hba.conf.tpl`;
  - cert placeholders: `config/postgres/certs/`, `config/minio/certs/`;
  - compose bindings: `AMI_POSTGRES_CERTS_DIR`, `AMI_MINIO_CERTS_DIR`, `AMI_MINIO_SCHEME`;
  - hardened contract теперь требует TLS keypair presence и `https` для S3/MinIO.
- operator UX cleanup:
  - operator commands теперь идут через `./scripts/amai_exec.sh` (binary-first);
  - build chatter подавляется по умолчанию, лог сохраняется в `state/logs/`.

### Queue 3: structural refactors

- Разрезать giant Rust modules по bounded contexts.
- Перевести giant SQL bootstrap в migration journal.
- Разделить dashboard/renderer and payload mapping.
- Перевести benchmark maturity из mapped/partial в measured/publicly reproducible contours.

Обновление по состоянию remediation:
- начат распил giant Rust modules:
  - MCP error/taxonomy слой вынесен в отдельный модуль `src/mcp_errors.rs`;
  - `src/mcp.rs` очищен от внутренних error-структур и helpers.
  - dashboard assets вынесены в `src/dashboard_assets.rs`.
  - dashboard formatting/тайминг helpers вынесены в `src/dashboard_format.rs`.
  - dashboard payload builders вынесены в `src/dashboard/dashboard_payload.rs`.
  - dashboard install/browser context helpers вынесены в `src/dashboard/dashboard_context.rs`.
  - dashboard card/status + monitoring URL helpers вынесены в `src/dashboard/dashboard_card_support.rs`.
  - dashboard renderer/template слой вынесен в `src/dashboard/dashboard_renderer.rs` + `src/dashboard/dashboard_template.html`; `src/dashboard.rs` больше не тащит встроенный HTML-монолит.
  - token-budget exact-client-limits cache/resolution contour вынесен в `src/token_budget/dashboard_exact_client_limits.rs`; `src/token_budget.rs` больше не держит рядом persisted schema, shared cache I/O и live resolution logic для этого dashboard-boundary.
  - token-budget shared hint/dedupe contour вынесен в `src/token_budget/dashboard_shared_hints.rs`; `src/token_budget.rs` больше не смешивает active-thread-hint и continuity-restore dedupe cache helpers с соседними dashboard cache lanes.
  - token-budget dashboard event caches вынесены в `src/token_budget/dashboard_event_caches.rs`; `src/token_budget.rs` больше не держит рядом persisted schema и shared cache I/O для token-events/current-session/live-turn-retrieval cache lanes.
  - token-budget dashboard event-cache runtime/orchestration слой вынесен в `src/token_budget/dashboard_event_cache_runtime.rs`; `src/token_budget.rs` больше не смешивает cache invalidation, merge/delta helpers и runtime cache orchestration с соседними contours, при этом `token_budget` module-surface сохранён через явный `pub(crate)` re-export для invalidation hooks.
  - token-budget dashboard statement preview/export contour вынесен в `src/token_budget/dashboard_statement_preview.rs`; `src/token_budget.rs` больше не держит рядом client-limit boundary review surface, dashboard read-only preview/export helpers и observed-whole-cycle assistant-scope projection helpers.
  - token-budget dashboard report surface assembly вынесен в `src/token_budget/dashboard_report_surface.rs`; `src/token_budget.rs` больше не держит вручную current-session preview helper и read-only statement surface bundle для dashboard report paths. По пути закрыт скрытый proof-fixture drift: minimal current-session reuse test fixture теперь materialize-ит валидный online-limit contour вместо слишком урезанного `status_bar_rate_limits`.
  - token-budget same-meter sync signature/shared-cache и dashboard report cache-debug/precache timing helpers вынесены в `src/token_budget/dashboard_report_cache_support.rs`; `src/token_budget.rs` больше не держит рядом same-meter sync cache policy, shared signature persistence и report cache-debug scaffolding.
  - token-budget assistant-scope source/scope orchestration contour вынесен в `src/token_budget/dashboard_assistant_scope.rs`; `src/token_budget.rs` больше не держит рядом assistant-scope persisted cache schema, shared cache I/O, source signature logic и debug surface. Во время выноса закрыт boundary-регресс: shared-cache path helper возвращён в production-visible surface вместо ошибочного `#[cfg(test)]`-сужения.
  - token-budget dashboard report core вынесен в `src/token_budget/dashboard_report_core.rs`; `src/token_budget.rs` больше не держит рядом report signature assembly, cache hit refresh/live-age update logic и in-process dashboard report cache store/load helpers. Во время выноса закрыт visibility-регресс: `dashboard_report_cache_support.rs` сохранил прямой доступ к component fields через `pub(super)`, без отката split.
  - token-budget current-session budget report contour вынесен в `src/token_budget/dashboard_current_session_report.rs`; `src/token_budget.rs` больше не держит current-session budget report orchestration, thread-bound snapshot fallback, restore-thread hint reuse и live-surface reuse helpers. Во время выноса закрыт module-surface регресс: `collect_dashboard_current_session_budget_report_with_thread_hint_and_base` и `collect_live_current_session_budget_guard` возвращены в `pub(crate)` re-export surface, а stale proof fixture обновлён до валидного online-limit contour вместо отката split.
  - token-budget active-agent live-budget contour вынесен в `src/token_budget/dashboard_active_agents.rs`; `src/token_budget.rs` больше не держит рядом active-agent selector/label/proof-runtime filter, per-agent personal limit surface assembly, dedupe/aggregate helpers и `collect_active_agent_live_budget_surface`. Во время выноса закрыт behavioural regression: label/tooltip contract для thread-local active-agent limit contour возвращён к прежнему виду (`Личный thread-limit агента`) вместо размытого global-limit текста.
  - token-budget live-response latency / current-thread file-hints contour вынесен в `src/token_budget/dashboard_live_response_latency.rs`; `src/token_budget.rs` больше не держит live-response latency scope assembly, relation annotation, current-thread file hint extraction и project-scoped surface builder. Во время выноса закрыт local supportability debt: test-only exported split helper `current_session_live_response_turns` получил `#[cfg_attr(not(test), allow(dead_code))]`, чтобы новый модуль не добавлял свежий warning-хвост.
  - token-budget agent-scope activity / recent-thread fallback contour вынесен в `src/token_budget/dashboard_agent_scope_activity.rs`; `src/token_budget.rs` больше не держит recent-scope fallback resolution, thread-key dedupe, active-agent activity entry assembly, `active_agent_thread_ids_from_activity` и `collect_agent_scope_activity`. Во время выноса закрыт module-surface регресс: `dashboard_active_agents.rs` снова импортирует `active_agent_activity_entries` из нового модуля вместо обращения к уже удалённому локальному symbol path.
  - token-budget active-agent support contour вынесен в `src/token_budget/active_agent_support.rs`; `src/token_budget.rs` больше не держит `PersonalKpiSelector`, workspace-personal selector resolution, per-agent KPI fallback window и связанные helper-правила рядом с чужими dashboard-срезами.
  - Queue 3 добит до owner-complete test layout: current-session budget report проверки переехали в `src/token_budget/dashboard_current_session_report.rs`, agent-scope activity/recent-thread fallback проверки в `src/token_budget/dashboard_agent_scope_activity.rs`, active-agent personal KPI fallback проверка в `src/token_budget/active_agent_support.rs`; `src/token_budget.rs` больше не держит чужие owner-tests для этих контуров.

## Acceptance Criteria For This Manual Verdict

Этот verdict-пакет считается завершённым, потому что:
- весь `amai_full_audit_report.md` пройден сверху вниз;
- у каждого существенного тезиса есть `verdict + evidence + risk + next step`;
- устаревшие пункты явно отделены от живых;
- remediation order не оставляет implementer-у решения, что чинить первым;
- live self-consistency defect, обнаруженный во время сверки, уже закрыт и повторно проверен через `amai status`.
