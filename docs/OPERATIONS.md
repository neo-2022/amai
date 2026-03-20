modified_at: 2026-03-20 16:54 MSK
Ручная сверка guide/docs: 2026-03-20 16:54 MSK

# Operations

Каноническое имя проекта:
- `Art-memory-agent-index`
- short name: `Amai`
- текущий path: `/home/art/agent-memory-index`

## Bootstrap

```bash
cd /home/art/agent-memory-index
cp .env.example .env
./scripts/bootstrap_stack.sh
```

Критичные `.env` поля:
- `AMI_DEFAULT_RETRIEVAL_MODE`
  - режим видимости по умолчанию;
- `AMI_LOCAL_FAST_CACHE_TTL_MS`
  - TTL для process-local hot cache;
  - увеличивать его без нужды не стоит, потому что слишком длинное окно хуже для реактивности на relation/config drift.

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
- SLA-оценку по [observability.toml](/home/art/agent-memory-index/config/observability.toml).

Сейчас hot retrieval stretch-goal в SLA считается только по реальному measured `p95_ms`, а не по округлению до целых миллисекунд.

Сейчас `observe sla-check` fail-ит только если:
- есть `critical` нарушение;
- или есть `unknown`, то есть обязательный контур ещё не был измерен.
