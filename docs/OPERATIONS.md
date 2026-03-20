modified_at: 2026-03-20 15:06 MSK
Ручная сверка guide/docs: 2026-03-20 15:06 MSK

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
