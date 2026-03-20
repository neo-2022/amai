modified_at: 2026-03-20 23:50 MSK
Ручная сверка guide/docs: 2026-03-20 23:50 MSK

# Operations

Каноническое имя проекта:
- `Art-memory-agent-index`
- short name: `Amai`
- текущий path: `/home/art/agent-memory-index`

## Bootstrap

Самый простой путь для локального пользователя:

```bash
cd /home/art/agent-memory-index
./scripts/install_amai.sh
```

Эта команда:
- сначала показывает понятную проверку машины;
- сама сравнивает два профиля установки;
- даёт выбрать `1` или `2`;
- если профиль слишком тяжёлый, печатает `ПРЕДУПРЕЖДЕНИЕ` и не идёт дальше молча;
- ждёт явного подтверждения словом `ДА`;
- создаёт и досинхронизирует `.env`;
- поднимает stack;
- materialize-ит bootstrap;
- собирает release binary;
- пишет готовый MCP config для клиента.

`install_amai.sh` делает ещё один шаг поверх этого:
- по умолчанию использует `client = auto`;
- пытается определить, какой клиент наиболее вероятен;
- работает как более человеческое имя для product install path.
- если запускать его повторно, он не должен плодить дубликаты, а должен аккуратно пересинхронизировать текущую установку.

Если нужен cheap remote/smoke contour под слабый VPS:

```bash
cd /home/art/agent-memory-index
./scripts/onboard_lite_vps.sh --client vscode
```

Этот путь:
- использует `stack_profile = lite_vps`;
- сначала делает Rust preflight;
- потом поднимает тот же baseline stack, но честно объясняет, что это не профиль для рекордных benchmark-цифр.

Симметричное удаление:

```bash
./scripts/remove_amai.sh
./scripts/remove_amai.sh --client codex
```

Если `Amai` уже живёт на удалённом Linux/VPS-host, а локально нужен только клиентский config:

```bash
cd /home/art/agent-memory-index
./scripts/onboard_remote_client.sh \
  --client vscode \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai
```

Этот путь:
- не поднимает локальный stack;
- не требует локального `docker compose up`;
- не требует локального `cargo build --release`;
- просто пишет клиентский config, который запускает удалённый `Amai` через `ssh`.

Список default client targets теперь хранится отдельно в:

```bash
config/client_targets.toml
```

Если нужен ручной инженерный путь:

```bash
cd /home/art/agent-memory-index
cp .env.example .env
./scripts/bootstrap_stack.sh
```

Критичные `.env` поля:
- `AMI_STACK_PROFILE`
  - machine-readable default profile для bootstrap/preflight;
  - сейчас канонические значения:
    - `default`
    - `lite_vps`;
- `AMI_DEFAULT_RETRIEVAL_MODE`
  - режим видимости по умолчанию;
- `AMI_LOCAL_FAST_CACHE_TTL_MS`
  - TTL для process-local hot cache;
  - увеличивать его без нужды не стоит, потому что слишком длинное окно хуже для реактивности на relation/config drift.
- `AMI_WARMUP_PROJECTS`
  - список project codes для автоматического warmup после bootstrap;
- `AMI_OBSERVE_BIND`
  - bind-адрес Rust exporter для Prometheus scrape;
- `AMI_PROMETHEUS_PORT` и `AMI_GRAFANA_PORT`
  - локальные порты monitoring profile.

## Warmup after bootstrap

Если cold-start нужно ускорить сразу после поднятия стека:

```bash
./scripts/warmup_cache.sh --projects project_alpha,project_beta
```

Если в `.env` задан `AMI_WARMUP_PROJECTS`, то:
- `bootstrap_stack.sh` сам вызовет `warmup_cache.sh`;
- warmup будет best-effort;
- незарегистрированные проекты будут честно перечислены в `skipped`, а bootstrap не сорвётся.

## Deployment profiles

Канонический registry профилей:

```bash
config/deployment_profiles.toml
```

Сейчас materialized два профиля:
- `default`
  - основной workstation/full baseline;
- `lite_vps`
  - cheap remote smoke/demo baseline.

Проверить машину под профиль:

```bash
./scripts/preflight.sh
./scripts/preflight.sh --stack-profile default
./scripts/preflight.sh --stack-profile lite_vps
```

Важно не путать:
- `install_amai.sh`
  - может перейти к реальной установке после выбора профиля и подтверждения;
- `preflight.sh`
  - ничего не устанавливает и ничего не меняет;
  - это режим только для проверки машины и выбора подходящего профиля.

Preflight показывает обычным человеческим языком:
- какой профиль выбран;
- minimum и recommended requirements;
- подходит ли машина под минимум;
- для чего профиль подходит;
- для чего профиль не подходит.

Принцип тут честный:
- `lite_vps` не скрывает ограничения;
- он специально нужен, чтобы пользователь сразу понимал границы ожиданий, а не узнал о них после неудачного benchmark-запуска.

## Status

```bash
./scripts/status.sh
```

Важно:
- для `Qdrant` и `NATS` канонический health source в этом проекте — не Docker health flag, а именно `status.sh` и `compat check`;
- это сделано специально, чтобы не зависеть от наличия `wget/curl/sh` внутри сторонних контейнерных образов.

## Compatibility check

```bash
cargo run -- compat check
```

Если здесь `FAIL`, дальше нельзя честно считать stack стабильным.
Сначала нужно убрать drift между поддерживаемым профилем и live версиями сервисов.

## Register a project

```bash
cargo run -- project register \
  --code project_alpha \
  --display-name "Project Alpha" \
  --repo-root /path/to/project-alpha
```

## Ensure a workspace inside the project

`namespace` здесь означает именованную рабочую область внутри проекта.
Она нужна для правил поиска и доступа.

```bash
cargo run -- namespace ensure \
  --project project_alpha \
  --code review \
  --display-name Review \
  --retrieval-mode local_strict
```

## Add a relation

```bash
cargo run -- relation add \
  --source project_alpha \
  --target project_beta \
  --relation-type shared_runtime \
  --shared-contour common_contour \
  --access-mode local_plus_related
```

## Index a project

```bash
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha \
  --namespace default
```

## Build a context pack

```bash
cargo run -- context pack \
  --project project_alpha \
  --namespace review \
  --query "how configuration is loaded" \
  --retrieval-mode local_strict
```

Результат:
- печатается в stdout как JSON;
- кэшируется в SQLite;
- сохраняется в PostgreSQL;
- выгружается в S3 context bucket.

Важно:
- `namespace` участвует в retrieval буквально;
- если вы запросили `default`, `Amai` не должен молча тянуть `smoke` или другой namespace того же проекта;
- если related project не имеет такого же namespace code, он просто не попадает в scope этого `context pack`.

## MCP server

Локальный MCP server:

```bash
cargo run -- mcp serve
```

Практический runner для IDE и других клиентов:

```bash
./scripts/run_mcp_stdio.sh
```

Этот runner:
- поднимает `.env`;
- не заставляет клиента дублировать внутренние credentials;
- стартует `amai mcp serve` как stdio MCP server.

## MCP client config

Сгенерировать client-specific snippet можно прямо из `Amai`:

```bash
cargo run -- mcp config --client generic
cargo run -- mcp config --client vscode --output .vscode/mcp.json
cargo run -- mcp config --client cursor
cargo run -- mcp config --client claude-code
cargo run -- mcp config --client claude-desktop
cargo run -- mcp config --client codex
```

Если нужен platform-specific launcher:

```bash
cargo run -- mcp config --client cursor --launcher-platform windows-powershell
cargo run -- mcp config --client codex --launcher-platform windows-cmd
```

Если `Amai` уже живёт на удалённом Linux/VPS-host:

```bash
cargo run -- mcp config \
  --client vscode \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai
```

Если auto-discovery корня не сработал:

```bash
cargo run -- mcp config --client vscode --cwd /path/to/art-memory-agent-index
```

Подробный user-facing walkthrough:
- [MCP_INTEGRATION.md](/home/art/agent-memory-index/docs/MCP_INTEGRATION.md)

## Onboarding

Если нужен один более простой вход, без ручной склейки шагов:

```bash
./scripts/onboard_local.sh --client vscode
./scripts/onboard_local.sh --client cursor
./scripts/onboard_local.sh --client codex
./scripts/onboard_local.sh --client claude-code
```

По умолчанию onboarding:
- работает внутри текущего repo root;
- пишет config в target path из `config/client_targets.toml`;
- для user-scope клиентов умеет создавать backup перед изменением файла.
- launcher platform тоже может быть указан явно:
  - `auto`
  - `linux`
  - `macos`
  - `windows-cmd`
  - `windows-powershell`

Текущие default outputs:
- `vscode` -> `.vscode/mcp.json`
- `cursor` -> `${home}/.cursor/mcp.json`
- `claude-code` -> `.mcp.json`
- `claude-desktop` -> `tmp/onboarding/claude-desktop-mcp.json`
- `codex` -> `${home}/.codex/config.toml`
- `generic` -> `tmp/onboarding/generic-mcp.json`

Proof:

```bash
./scripts/proof_install_auto.sh
./scripts/proof_onboarding.sh
./scripts/proof_remote_onboarding.sh
./scripts/proof_client_lifecycle.sh
./scripts/proof_profiles.sh
```

## Disconnect

Симметричное удаление клиента:

```bash
./scripts/disconnect_local.sh --client vscode
./scripts/disconnect_local.sh --client cursor
./scripts/disconnect_local.sh --client codex
./scripts/disconnect_local.sh --client claude-code
```

При disconnect:
- удаляется только запись `Amai`, а не весь чужой config целиком;
- если файл после этого становится пустым и включён `purge_empty_file`, пустой файл удаляется;
- для user-scope config перед изменением создаётся backup.

## Platform launchers

Materialized runner files:

```text
scripts/run_mcp_stdio.sh
scripts/run_mcp_stdio.ps1
scripts/run_mcp_stdio.cmd
```

Это значит:
- Linux/macOS path можно обслуживать shell launcher'ом;
- Windows path можно обслуживать через `cmd` или `PowerShell`;
- client config generation теперь умеет честно учитывать platform launcher, а не только Unix-style путь.
- удалённый Linux/VPS-host теперь можно подключать через `ssh` как stdio-transport, не выставляя внутренние базы наружу.

## Hardening proof

Быстрый локальный proof:

```bash
./scripts/proof_local.sh
```

Более жёсткий proof:

```bash
./scripts/proof_hardening.sh
```

Он дополнительно проверяет:
- повторный bootstrap;
- compatibility profile;
- multi-project isolation на fixture-проектах;
- controlled cross-project reading;
- restart recovery после `docker compose restart`.

## Performance proof

```bash
./scripts/proof_performance.sh
```

Этот proof:
- индексирует fixture-проекты с эмбеддингами;
- гоняет и `hot`, и `cold` retrieval path;
- мерит `mean/p50/p95/p99/max`;
- считает hot-path в микросекундах и публикует его как дробные миллисекунды;
- fail-ит, если practical latency baseline выходит за заданные thresholds.

Прямая Rust-команда:

```bash
cargo run --release -- verify benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --warmup 1 \
  --iterations 5 \
  --persist
```

Для cold-path добавляется:

```bash
cargo run --release -- verify benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --disable-cache \
  --warmup 1 \
  --iterations 5 \
  --persist
```

Важно:
- без `--disable-cache` измеряется `hot retrieval`;
- с `--disable-cache` измеряется `cold retrieval`.

Текущий репозиторный guard:
- hot benchmark должен удерживать `p95 < 10ms`
- hot benchmark должен удерживать `max < 15ms`

## Accuracy proof

```bash
./scripts/proof_accuracy.sh
```

Или напрямую:

```bash
cargo run --release -- verify accuracy \
  --project project_alpha \
  --related-project project_beta \
  --namespace review
```

Этот proof:
- проверяет `local_strict` на отсутствие cross-project leakage;
- проверяет отсутствие cross-namespace leakage внутри одного и того же проекта;
- мерит `exact_precision`, `lexical_precision`, `symbol_precision`, `semantic_precision`;
- сохраняет snapshot `retrieval_accuracy`.

## Load proof

```bash
./scripts/proof_load.sh
```

Или напрямую:

```bash
cargo run --release -- verify load \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --workers 2 \
  --iterations-per-worker 25
```

Этот proof:
- мерит concurrent hot-load contour;
- выдаёт `qps`, `error_rate`, `p50/p95/p99/max`;
- сохраняет snapshot `retrieval_load_hot`.

Текущий репозиторный guard:
- `qps >= 5000`
- `p95 < 10ms`
- `error_rate = 0`

Важно:
- если после warmup `verify load` пишет `execution_mode = hot_cache_only`, это правильный и желаемый режим;
- в этом режиме verifier честно мерит process-local hot retrieval, а не открывает PostgreSQL connection на каждого worker;
- возврат к per-worker DB connections для hot-load считается регрессом, даже если код “выглядит проще”.

## Stress scale proof

```bash
./scripts/proof_stress_scale.sh
```

Этот proof:
- поднимает fixture stack;
- прогревает hot cache;
- последовательно гоняет `50`, `100` и `200` workers;
- fail-ит, если `p95 >= 10ms`, `qps < 5000` или появляется `error_rate`.

Текущий честный measured baseline на референсной машине:
- CPU:
  - `AMD Ryzen 9 7900X 12-Core Processor`
  - `24` логических CPU
- RAM:
  - `62 GiB`
- `50 workers`
  - `p95 = 0.026 ms`
  - `qps ≈ 384 024`
- `100 workers`
  - `p95 = 0.023 ms`
  - `qps ≈ 434 593`
- `200 workers`
  - `p95 = 0.020 ms`
  - `qps ≈ 670 016`

Эти цифры относятся именно к `hot cached retrieval`.
Cold/full path нужно оценивать отдельно через `proof_performance.sh` и при необходимости заранее прогревать `warmup_cache.sh`.

## Token benchmark proof

```bash
./scripts/proof_token_benchmark.sh
./scripts/proof_token_benchmark_suite.sh
```

Или напрямую:

```bash
cargo run --release -- verify token-benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --tokenizer o200k_base
```

Этот proof:
- считает, сколько токенов потребовал бы наивный полный scope без retrieval reduction;
- строит компактный LLM-ready render текущего `context pack`;
- сравнивает оба результата на одном tokenizer;
- сохраняет snapshot `token_benchmark`.

## Token benchmark suite proof

```bash
./scripts/proof_token_benchmark_suite.sh
```

Или напрямую:

```bash
cargo run --release -- verify token-benchmark-suite \
  --project project_alpha \
  --namespace review \
  --retrieval-mode local_plus_related \
  --queries-file "$PWD/fixtures/token_benchmark_queries.txt" \
  --tokenizer o200k_base \
  --naive-limit-files 20 \
  --naive-max-bytes-per-file 32768 \
  --min-mean-savings-factor 1.2 \
  --min-mean-savings-percent 15
```

Этот proof:
- берёт список запросов, а не один удачный пример;
- строит агрегированный `token_benchmark_suite` snapshot;
- считает `mean/p50/p95` по `saved_tokens`, `savings_factor`, `savings_percent`;
- нужен как более честный product contour, который другой пользователь сможет воспроизвести на том же fixture наборе.

## MCP proof

```bash
./scripts/proof_mcp.sh
```

Отдельный proof для удалённого `ssh` config generation:

```bash
./scripts/proof_remote_ssh_config.sh
```

Сравнительный text contour:

```bash
./scripts/proof_text_compare.sh
```

Ручной запуск comparative benchmark:

```bash
cargo run -- verify text-compare \
  --project project_alpha \
  --namespace review \
  --retrieval-mode local_plus_related \
  --cases-file fixtures/text_compare_cases.jsonl
```

Или напрямую:

```bash
cargo run --release -- verify mcp \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related
```

Этот proof:
- поднимает child MCP server;
- проходит `initialize`;
- проверяет `tools/list`;
- проверяет `prompts/list` и `prompts/get`;
- вызывает через MCP:
  - `amai_list_projects`
  - `amai_list_namespaces`
  - `amai_context_pack`
  - `amai_token_benchmark`
  - `amai_observe_snapshot`
  - `amai_warm_cache`.

Важно:
- на маленьких fixture-проектах экономия токенов будет честно умеренной;
- на больших реальных репозиториях этот contour должен расти заметно сильнее;
- proof нужен именно затем, чтобы показывать пользователю measured effect, а не обещание.

## Hostile proof

```bash
./scripts/proof_hostile.sh
```

Этот proof:
- специально создаёт `stack_meta` drift;
- по очереди выключает `postgres`, `qdrant`, `minio`, `nats`;
- проверяет, что compatibility path fail-closed;
- затем поднимает сервис обратно и доказывает recovery.

Прямая Rust-команда:

```bash
cargo run -- verify hostile --scenario all
```

Допустимые точечные сценарии:
- `stack_meta_drift`
- `postgres`
- `qdrant`
- `minio`
- `nats`

Текущий AST coverage:
- `rust`
- `toml`
- `javascript`
- `typescript`
- `tsx`
- `json`

Если файл попадает вне этого набора, индексер обязан перейти в lexical fallback, а не валить весь проход.

Для smoke-проверки:

```bash
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha/src \
  --namespace review \
  --limit-files 10
```

Быстрый smoke без эмбеддингов:

```bash
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha/src \
  --namespace review \
  --limit-files 5 \
  --skip-embeddings
```

## Observability / SLA proof

```bash
./scripts/proof_observability.sh
```

Или напрямую:

```bash
cargo run --release -- observe snapshot
cargo run --release -- observe sla-check
```

Что это даёт:
- live snapshot по `PostgreSQL`, `Qdrant`, `NATS`, `S3-compatible storage`;
- последние `index_project` и `retrieval_benchmark` snapshots;
- последние `retrieval_accuracy` и `retrieval_load_hot` snapshots;
- последний `token_benchmark` snapshot;
- SLA-оценку по [observability.toml](/home/art/agent-memory-index/config/observability.toml).
- Prometheus-ready `/metrics` exporter без persistence на каждый scrape.

Сейчас hot retrieval stretch-goal в SLA считается только по реальному measured `p95_ms`, а не по округлению до целых миллисекунд.

Сейчас `observe sla-check` fail-ит только если:
- есть `critical` нарушение;
- или есть `unknown`, то есть обязательный контур ещё не был измерен.

## Monitoring profile

Встроенный exporter:

```bash
./scripts/run_observe_exporter.sh
```

Prometheus + Grafana:

```bash
./scripts/render_monitoring_config.sh
./scripts/monitoring_up.sh
```

После этого доступны:
- `Prometheus`: `http://127.0.0.1:${AMI_PROMETHEUS_PORT:-59090}`
- `Grafana`: `http://127.0.0.1:${AMI_GRAFANA_PORT:-53000}`

Канонические файлы:
- [config/prometheus/prometheus.yml](/home/art/agent-memory-index/config/prometheus/prometheus.yml)
- [config/prometheus/rules/alerts.yml](/home/art/agent-memory-index/config/prometheus/rules/alerts.yml)
- [config/grafana/dashboards/amai_stack.json](/home/art/agent-memory-index/config/grafana/dashboards/amai_stack.json)
- [scripts/render_monitoring_config.sh](/home/art/agent-memory-index/scripts/render_monitoring_config.sh)

Базовые алерты уже materialized:
- `AmaiQdrantIndexOptimizeQueueHigh`
- `AmaiNatsConsumerLagHigh`
- `AmaiPostgresReplicaLagHigh`
- `AmaiRetrievalHotBudgetMiss`
- `AmaiCrossProjectLeakageDetected`
- `AmaiPostgresDeadlocksDetected`

Ключевой engineering law:
- scrape path не должен менять operational truth;
- поэтому `/metrics` собирает live snapshot read-only и не пишет `system_snapshot` в PostgreSQL на каждый Prometheus scrape;
- persistence остаётся только у явных `observe snapshot` и `observe sla-check`.
- runtime scrape targets и monitoring ports не должны быть вшиты в конфиг как абсолютные литералы;
- поэтому monitoring profile рендерится из `.env` перед `docker compose --profile monitoring up`.
- token-economy metrics тоже приходят в exporter из последнего `token_benchmark` snapshot:
  - `amai_tokens_naive_scope_total`
  - `amai_tokens_context_pack_total`
  - `amai_tokens_saved_total`
  - `amai_tokens_savings_factor`
  - `amai_tokens_savings_percent`

## Hardware baseline

Текущий репозиторный latency/load baseline materialized на таком host:
- CPU: `AMD Ryzen 9 7900X`
- `12c / 24t`
- RAM: `62 GiB`
- storage: `NVMe HS-SSD-G4000 2048G`
- architecture: `x86_64`

Повторная проверка другими инженерами должна делаться:
- на железе не хуже;
- теми же proof-командами;
- с тем же разделением `hot` и `cold` contours.
