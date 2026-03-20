modified_at: 2026-03-20 19:57 MSK
Ручная сверка guide/docs: 2026-03-20 19:57 MSK

# Art-memory-agent-index (Amai)

![Amai lockup](brand/amai_lockup.svg)

Amai — это отдельный внешний инструмент для ИИ-агентов.
Он помогает агентам работать сразу с несколькими репозиториями и при этом не путать их между собой.

Проще говоря, Amai делает четыре вещи:
- запоминает, с каким проектом сейчас работает агент;
- индексирует код и документы так, чтобы их можно было быстро находить;
- собирает для агента готовую подборку полезного контекста по запросу;
- не даёт по умолчанию смешивать данные разных проектов.

Это не плагин для одной IDE.
Это самостоятельный backend/tooling проект, который можно использовать из VS Code, Cursor, JetBrains, CLI или другого agent runtime.

## Это не просто общий чат

Важно понять главную идею:

`Amai` не делает “один бесконечный общий чат”.
Он делает для агента постоянное рабочее пространство с памятью и правилами.

Разница простыми словами:
- обычный чат
  - почти каждый новый запуск начинается заново;
  - важные вещи приходится повторять;
  - один проект легко перепутать с другим;
- `Amai`
  - агент поднимает уже существующий рабочий контур;
  - знает, с каким проектом он сейчас работает;
  - знает, что нужно видеть всегда, а что нужно искать по запросу;
  - не смешивает проекты по умолчанию.

Хорошая бытовая аналогия:
- обычный чат — это случайный исполнитель, которому каждый раз заново всё объясняют;
- `Amai` — это постоянный помощник, у которого есть:
  - рабочий стол;
  - шкаф с архивом;
  - общая доска для команды;
  - быстрый поиск по коду и документам.

## Как память разложена по полочкам

Чтобы не путаться, полезно представлять `Amai` так:

- `память на столе`
  - это короткие важные правила и текущие рабочие вводные;
  - агент видит их сразу;
  - сюда не кладут весь проект целиком.
- `архив`
  - это большой запас материалов, который не нужно держать перед глазами всё время;
  - туда попадают документы, код, история решений, артефакты;
  - агент идёт туда только когда это действительно нужно.
- `общая доска`
  - это память, которой могут пользоваться несколько агентов;
  - один агент обновил важное правило, другие это сразу увидели.
- `готовая подборка`
  - это уже собранный пакет нужных материалов под конкретный запрос;
  - агент получает не весь проект, а только полезный контекст для следующего шага.

Именно поэтому `Amai` — это не просто “вечный чат”, а “долгоживущая память с порядком”.

## Что это даёт обычному человеку

Если говорить без инженерного жаргона, `Amai` нужен затем, чтобы:
- не повторять одно и то же ИИ снова и снова;
- не путать один проект с другим;
- не тратить лишние токены на повторный ввод контекста;
- быстрее получать полезную подборку материалов;
- сохранять важные решения между сессиями;
- подключать один и тот же внешний инструмент к разным IDE и агентам.

## Что означают ключевые термины

- `проект`
  - отдельный репозиторий или рабочий корень, который агент должен видеть как самостоятельную сущность;
- `рабочая область проекта` (`namespace`)
  - именованная зона внутри проекта для правил поиска и доступа;
- `поиск контекста` (`retrieval`)
  - поиск нужных документов, символов, фрагментов кода и связанных материалов;
- `поиск по смыслу` (`semantic search`)
  - поиск не по точному совпадению слов, а по похожему смыслу;
- `готовая подборка контекста` (`context pack`)
  - собранный пакет найденных материалов с указанием происхождения каждого фрагмента;
- `provenance`
  - явное указание, из какого проекта, файла и места в коде пришёл фрагмент.
- `MCP`
  - общий стандарт, через который IDE и ИИ-клиенты могут подключаться к внешнему инструменту как к серверу возможностей.

Клиентами могут быть:
- VS Code;
- Cursor;
- JetBrains IDE;
- CLI-агенты;
- CI;
- web UI;
- локальные orchestrators.

## Стек

- `PostgreSQL`
- `Qdrant`
- `S3-compatible object storage`
- `NATS Core + JetStream`
- `tree-sitter`
- `SQLite edge cache`
- `LanceDB` только optional на edge
- `Milvus` только как future scale-up replacement path
- `config/observability.toml` как machine-readable SLA / observability профиль

## Parser Baseline

Текущий code-structure слой materialize-ится не через агрегирующий pack, а через прямые Rust grammar crates поверх `tree-sitter`.

Сейчас реальный AST/symbol path покрывает:
- `rust`
- `toml`
- `javascript`
- `typescript`
- `tsx`
- `json`

Для остальных расширений проект пока делает честный `lexical-only fallback`, не ломая весь ingest.

## Карта Файлов Текущего Уровня

- `AGENTS.md`
  - обязательный вход для любого нового ИИ.
- `README.md`
  - краткий вход и карта проекта.
- `Cargo.toml`
  - Rust package, зависимости и бинарь `amai`.
- `compose.yaml`
  - локальный runnable stack.
- `.env.example`
  - обязательный конфигурационный шаблон.

## Карта Поддоменов

- `brand/`
  - канонический branding contour проекта: lockup, mark, favicon и brand spec.
- `docs/`
  - подробная архитектура, схема данных, операции и lifecycle.
- `config/`
  - конфиги сервисов, compatibility profile и machine-readable client target registry.
- `sql/`
  - каноническая схема PostgreSQL и seed-данные.
- `scripts/`
  - bootstrap, status и helper wrappers.
- `fixtures/`
  - нейтральные маленькие проекты для hardening и recovery proof.
- `src/`
  - Rust CLI и runtime bootstrap/index logic.
- `docs/MCP_INTEGRATION.md`
  - простой вход для подключения `Amai` к MCP-клиентам.
- `tests/`
  - локальные smoke и unit checks.
- `state/`
  - локальные данные контейнеров, не трекаются в git.
- `tmp/`
  - временные runtime-артефакты.

## Branding

Brand-pack проекта теперь хранится прямо в repo:
- [brand/README.md](/home/art/agent-memory-index/brand/README.md)
- [brand/amai_lockup.svg](/home/art/agent-memory-index/brand/amai_lockup.svg)
- [brand/amai_mark.svg](/home/art/agent-memory-index/brand/amai_mark.svg)
- [brand/favicon.ico](/home/art/agent-memory-index/brand/favicon.ico)
- [brand/amai_brand_spec.md](/home/art/agent-memory-index/brand/amai_brand_spec.md)

Правило использования:
- `README` и docs используют lockup;
- favicon и compact icon используют square mark или `favicon.ico`.

## Самый простой старт

Если вам нужен путь “запустить как можно проще”, используйте onboarding-команду:

```bash
cd /home/art/agent-memory-index
./scripts/onboard_local.sh --client vscode
```

Что она делает сама:
- создаёт `.env`, если его ещё нет;
- досинхронизирует недостающие переменные из `.env.example`;
- поднимает локальный stack;
- прогоняет bootstrap схемы и служебных слоёв;
- собирает `release` binary;
- создаёт готовый MCP config для выбранного клиента.

Для `VS Code` это почти путь “поднял и пользуйся”:
- onboarding пишет config в `.vscode/mcp.json`;
- потом обычно достаточно открыть repo в VS Code и сделать `Reload Window`.

Для других клиентов onboarding тоже упрощает работу:
- `Cursor` по умолчанию получает auto-install в user config;
- `Codex` по умолчанию получает auto-install в user config;
- `Claude Code` получает workspace-local `.mcp.json`;
- `Claude Desktop` и `generic` пока получают готовый generated file для ручного импорта.

Примеры:

```bash
./scripts/onboard_local.sh --client vscode
./scripts/onboard_local.sh --client cursor
./scripts/onboard_local.sh --client codex
./scripts/disconnect_local.sh --client codex
```

## Инженерный старт вручную

Если вы хотите пройти тот же путь по шагам и видеть каждое действие отдельно:

1. Скопировать `.env.example` в `.env`
2. Запустить локальный стек:

```bash
cd /home/art/agent-memory-index
./scripts/bootstrap_stack.sh
```

Что важно проверить в `.env` сразу:
- `AMI_DEFAULT_RETRIEVAL_MODE`
  - базовый режим изоляции по умолчанию;
- `AMI_LOCAL_FAST_CACHE_TTL_MS`
  - окно жизни process-local hot cache в миллисекундах;
  - этот cache ускоряет повторные `context pack` запросы, но не заменяет PostgreSQL, SQLite и S3 persistence.
- `AMI_WARMUP_PROJECTS`
  - необязательный список уже зарегистрированных project codes для автоматического cold-start warmup после `bootstrap_stack.sh`;
- `AMI_OBSERVE_BIND`
  - bind-адрес встроенного Rust exporter для Prometheus scrape.

3. Проверить, что всё поднялось:

```bash
./scripts/status.sh
```

Если cold-start нужно прогреть сразу после bootstrap:

```bash
./scripts/warmup_cache.sh --projects project_alpha,project_beta
```

Важно:
- пример выше использует условные project codes;
- bootstrap не пришит к конкретным продуктам;
- автоматический warmup сработает только если в `.env` задан `AMI_WARMUP_PROJECTS` и такие проекты уже зарегистрированы;
- если проектов ещё нет, bootstrap честно пропустит warmup и продолжит поднимать stack.

Дополнительно можно прогнать:

```bash
./scripts/proof_local.sh
./scripts/proof_hardening.sh
./scripts/proof_performance.sh
./scripts/proof_accuracy.sh
./scripts/proof_load.sh
./scripts/proof_hostile.sh
./scripts/proof_token_benchmark.sh
./scripts/proof_observability.sh
./scripts/proof_mcp.sh
./scripts/proof_onboarding.sh
./scripts/proof_client_lifecycle.sh
```

4. Зарегистрировать свои проекты:

```bash
cargo run -- project register --code project_alpha --display-name "Project Alpha" --repo-root /path/to/project-alpha
cargo run -- project register --code project_beta --display-name "Project Beta" --repo-root /path/to/project-beta
cargo run -- relation add --source project_alpha --target project_beta --relation-type shared_runtime --shared-contour common_contour --access-mode local_plus_related
```

Или после первого `cargo build`:

```bash
./target/debug/amai project register --code project_alpha --display-name "Project Alpha" --repo-root /path/to/project-alpha
```

## Подключение через MCP

`Amai` теперь materialize-ит и собственный MCP server.
Это значит, что совместимые клиенты могут запрашивать у него:
- список зарегистрированных проектов;
- namespace внутри проекта;
- context pack;
- token benchmark;
- observability snapshot;
- warmup cache.

Минимальный путь:
1. самый простой путь:
   - `./scripts/onboard_local.sh --client vscode`
2. если нужен ручной путь:
   - поднять stack через `./scripts/bootstrap_stack.sh`
   - собрать release binary:

```bash
cargo build --release
```

3. сгенерировать config snippet для нужного клиента:

```bash
./target/release/amai mcp config --client vscode --output .vscode/mcp.json
./target/release/amai mcp config --client cursor
./target/release/amai mcp config --client claude-code
./target/release/amai mcp config --client claude-desktop
./target/release/amai mcp config --client codex
```

Что важно:
- клиенту не нужно хранить DSN, bucket names и другие внутренние runtime детали;
- клиент запускает `Amai` через `scripts/run_mcp_stdio.sh`;
- runner сам подтягивает `.env` и стартует `amai mcp serve`.
- список default install targets теперь живёт не в коде README, а в:
  - `config/client_targets.toml`

## Подключение и удаление

`Amai` теперь умеет не только подключать client config, но и убирать его обратно.

Примеры:

```bash
./scripts/onboard_local.sh --client vscode
./scripts/onboard_local.sh --client cursor
./scripts/onboard_local.sh --client codex

./scripts/disconnect_local.sh --client vscode
./scripts/disconnect_local.sh --client cursor
./scripts/disconnect_local.sh --client codex
```

Это важно по двум причинам:
- обычному пользователю не нужно потом руками вычищать куски config;
- install/remove превращается в симметричный product lifecycle, а не в одноразовый setup без обратного пути.

Подробный human-readable walkthrough:
- [docs/MCP_INTEGRATION.md](/home/art/agent-memory-index/docs/MCP_INTEGRATION.md)

## Retrieval law

Правильный retrieval order:
1. определить активный проект;
2. понять, можно ли смотреть только его или ещё связанные проекты;
3. найти точные совпадения в Postgres;
4. найти подходящие символы через tree-sitter;
5. найти похожие по смыслу куски через Qdrant;
6. собрать готовую подборку контекста с источником каждого фрагмента.

Важно:
- unsupported parser language не должен валить индексирующий проход целиком;
- сначала сохраняется lexical/provenance baseline, затем по мере появления grammar coverage расширяется AST contour.
- запрошенный `namespace` внутри проекта тоже является частью границы поиска:
  - `default` не должен молча подтягивать результаты из `smoke`;
  - related project участвует только если у него есть такой же `namespace` code.

## Context Pack

`Amai` теперь materialize-ит не только indexing, но и agent-facing retrieval/context-pack contour:

```bash
cargo run -- context pack \
  --project project_alpha \
  --namespace review \
  --query "how configuration is loaded" \
  --retrieval-mode local_strict
```

Команда:
- делает exact lookup по documents;
- делает symbol lookup;
- делает lexical chunk lookup;
- сначала пытается сделать semantic chunk recall через Qdrant;
- если vector tier временно не даёт usable hits, честно деградирует в lexical fallback вместо пустого semantic слоя;
- если нет exact/symbol/lexical evidence и semantic hits не перекрывают query terms по path/content, `Amai` честно возвращает пустой semantic слой вместо слабого шума;
- собирает provenance-rich context pack;
- пишет его в PostgreSQL, SQLite edge cache и S3 context bucket.

## Verification contour

В проекте теперь есть отдельный verification layer, а не только smoke-скрипты.

Прямые Rust-команды:

```bash
cargo run -- verify benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --warmup 1 \
  --iterations 5 \
  --persist

cargo run -- verify accuracy \
  --project project_alpha \
  --related-project project_beta \
  --namespace review

cargo run -- verify load \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --workers 2 \
  --iterations-per-worker 25

cargo run -- verify token-benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --tokenizer o200k_base

cargo run -- verify token-benchmark-suite \
  --project project_alpha \
  --namespace review \
  --retrieval-mode local_plus_related \
  --queries-file fixtures/token_benchmark_queries.txt \
  --tokenizer o200k_base

cargo run -- verify hostile --scenario all
```

Что они доказывают:
- `verify benchmark`
  - мерит живой `context pack` path по времени;
  - выдаёт `mean/p50/p95/max`;
  - считает время в микросекундах и публикует его как дробные миллисекунды, чтобы быстрый hot-path не схлопывался в ложный `0ms`;
  - может fail-ить при нарушении заданных latency thresholds;
- `verify hostile`
  - проверяет fail-closed реакцию на partial-service loss;
  - проверяет recovery после возврата сервиса;
  - отдельно проверяет drift в `stack_meta`.
- `verify accuracy`
  - доказывает `cross_project_leakage = 0`;
  - мерит `symbol_precision` и `semantic_precision`;
  - сохраняет snapshot `retrieval_accuracy`.
- `verify load`
  - мерит concurrent hot-load contour;
  - выдаёт `qps`, `error_rate`, `p50/p95/p99/max`;
  - сохраняет snapshot `retrieval_load_hot`.
- `verify token-benchmark`
  - мерит, сколько токенов потребовал бы наивный полный scope без retrieval reduction;
  - сравнивает это с компактным LLM-ready render текущего `context pack`;
  - сохраняет snapshot `token_benchmark`;
  - даёт продуктовую цифру реальной экономии контекста для пользователя.
- `verify token-benchmark-suite`
  - гоняет не один запрос, а список типовых запросов на одном и том же stack contour;
  - считает `mean/p50/p95` по `saved_tokens`, `savings_factor`, `savings_percent`;
  - сохраняет snapshot `token_benchmark_suite`;
  - нужен для более честного product proof, который другой инженер сможет повторить не на одной удачной фразе, а на серии запросов.

Текущий materialized guardrail:
- `hot retrieval p95 < 10ms`
- `concurrent hot-load p95 < 10ms`
- `concurrent hot-load qps >= 5000`
- `cross_project_leakage = 0`

## Observability contour

Теперь в проекте materialized и отдельный observability/SLA слой:

```bash
cargo run --release -- observe snapshot
cargo run --release -- observe sla-check
./scripts/run_observe_exporter.sh
./scripts/monitoring_up.sh
```

Что он делает:
- снимает live snapshot по `PostgreSQL`, `Qdrant`, `NATS` и `S3-compatible storage`;
- подтягивает последние benchmark/index snapshots из PostgreSQL;
- считает SLA-статусы по machine-readable профилю [observability.toml](/home/art/agent-memory-index/config/observability.toml);
- отделяет `hot retrieval` от `cold retrieval`, чтобы не подменять одно другим.
- публикует Prometheus metrics через встроенный Rust exporter, не делая write-side persistence на каждый scrape.

Сейчас snapshot показывает как минимум:
- `PostgreSQL`
  - `connection_usage_ratio`
  - `query_probe_p95_ms`
  - `transactions_total`
  - `deadlocks_total`
  - `wal_bytes_total`
- `Qdrant`
  - `collections_vector_total`
  - `running_optimizations`
  - `update_queue_length`
  - `memory_resident_bytes`
  - cold retrieval `semantic_search_ms p95` через последний cold benchmark
- `NATS / JetStream`
  - `publish_probe_p95_ms`
  - `consumer_lag_msgs`
  - `jetstream_disk_usage_ratio`
- `Retrieval`
  - отдельные `hot` и `cold` benchmark snapshots
- `Indexing`
  - `files_per_min`
  - `parser_coverage_ratio`
  - `language_breakdown`
- `Accuracy`
  - `cross_project_leakage`
  - `symbol_precision`
  - `semantic_precision`
- `Load`
  - `hot_qps`
  - `hot_error_rate`

Production monitoring profile materialized в repo:
- [config/prometheus/prometheus.yml](/home/art/agent-memory-index/config/prometheus/prometheus.yml)
- [config/prometheus/rules/alerts.yml](/home/art/agent-memory-index/config/prometheus/rules/alerts.yml)
- [config/grafana/dashboards/amai_stack.json](/home/art/agent-memory-index/config/grafana/dashboards/amai_stack.json)
- [scripts/render_monitoring_config.sh](/home/art/agent-memory-index/scripts/render_monitoring_config.sh)

Ключевые runtime metrics, которые теперь есть в `/metrics`:
- `amai_qdrant_index_optimize_queue`
- `amai_nats_consumer_lag_msgs`
- `amai_postgres_replica_lag_seconds`
- `amai_retrieval_hot_p95_ms`
- `amai_retrieval_cold_p95_ms`
- `amai_load_hot_qps`
- `amai_parser_coverage_ratio`
- `amai_accuracy_cross_project_leakage`
- `amai_tokens_naive_scope_total`
- `amai_tokens_context_pack_total`
- `amai_tokens_saved_total`
- `amai_tokens_savings_factor`
- `amai_tokens_savings_percent`

Важно:
- monitoring config не должен держать runtime ports/targets в жёстких литералах;
- поэтому Prometheus/Grafana datasource config рендерятся из `.env` через `render_monitoring_config.sh` перед запуском monitoring profile.

Важно:
- `hot retrieval` означает работающий result-cache contour;
- `cold retrieval` означает живой retrieval path без result-cache bypassing;
- быстрый hot-path в `Amai` опирается на process-local fast cache с TTL из `.env`, но не отменяет durable persistence в PostgreSQL, SQLite edge cache и S3;
- оба режима нужны одновременно, иначе нельзя честно оценить ни UX-скорость, ни реальную цену полного retrieval path.

## Benchmark hardware baseline

Текущие репозиторные цифры были materialized на локальном single-node host:
- CPU: `AMD Ryzen 9 7900X`
- topology: `12` физических ядер / `24` потока
- max clock: `~5.7 GHz`
- RAM: `62 GiB`
- storage: `NVMe HS-SSD-G4000 2048G`
- architecture: `x86_64`

Это важно:
- hot/cold цифры нужно сравнивать только с тем же proof contour;
- если другой инженер запускает те же команды на железе не хуже, результаты должны подтверждаться в том же порядке величин;
- scrape path и monitoring path специально отделены от hot retrieval path, чтобы observability не размывала latency baseline.

## Защита от version drift

В `Amai` есть отдельный compatibility contour:
- machine-readable профиль: [compatibility.toml](/home/art/agent-memory-index/config/compatibility.toml)
- live проверка:

```bash
cargo run -- compat check
```

Инструмент fail-closed ловит несовместимый drift по:
- `PostgreSQL`
- `Qdrant`
- `NATS`
- `stack_meta` schema/profile state

Для S3-compatible слоя сейчас удерживается API-совместимость и family-check без жёсткой блокировки по vendor string.

## Быстрый индексирующий smoke

```bash
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha/src \
  --namespace review \
  --limit-files 5 \
  --skip-embeddings
```
