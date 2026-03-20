modified_at: 2026-03-20 16:33 MSK
Ручная сверка guide/docs: 2026-03-20 16:33 MSK

# AGENTS.md для Art-memory-agent-index (Amai)

## 1. Что это

`Art-memory-agent-index (Amai)` в текущем filesystem path `/home/art/agent-memory-index` — отдельный standalone проект.

Это не код одного конкретного продукта.
Это внешний инструмент для ИИ-агентов, который:
- хранит рабочий контекст между сессиями;
- знает, где начинается и заканчивается каждый проект;
- умеет искать по коду и документам;
- умеет собирать готовый пакет контекста для следующего шага агента;
- не даёт по умолчанию смешивать разные проекты.

## 2. Обязательный старт

Любой новый агент обязан:
1. сначала прочитать этот `AGENTS.md`;
2. затем прочитать `README.md`;
3. затем прочитать `docs/ARCHITECTURE.md`;
4. затем прочитать `docs/OPERATIONS.md`;
5. только после этого трогать compose/config/schema/code.

Если стек уже запущен, сначала проверить:
- `scripts/status.sh`

Если стек ещё не materialized:
- `scripts/bootstrap_stack.sh`

## 3. Главный закон

Этот проект существует для того, чтобы агенты не смешивали проекты по умолчанию.

Значит:
- новый `repo_root` считается отдельным проектом;
- пока он не зарегистрирован, поиск контекста по нему запрещён;
- cross-project reading допускается только через relation graph и policy modes.

## 4. Канонический порядок слоёв

1. `PostgreSQL` — главный источник истины: проекты, правила доступа, метаданные и точный поиск
2. `Qdrant` — поиск похожих по смыслу фрагментов
3. `S3-compatible object storage` — хранение артефактов и готовых пакетов контекста
4. `NATS Core + JetStream` — события и рабочие очереди
5. `tree-sitter` — структура кода, символы и границы сущностей
6. `SQLite` — локальный быстрый кэш агента
7. `LanceDB` — только optional local semantic edge cache

CLI/binary canonical short name:
- `amai`

Текущий parser baseline materialized напрямую через отдельные Rust grammar crates.
Реальный AST/symbol contour сейчас есть для:
- `rust`
- `toml`
- `javascript`
- `typescript`
- `tsx`
- `json`

Для остальных форматов новый агент обязан понимать, что сейчас допустим только lexical fallback, а не молчаливо считать coverage полным.

## 5. Что запрещено

- превращать IDE в источник истины;
- заменять lexical/symbol retrieval только embeddings-поиском;
- подмешивать чужой проект “по похожести”;
- считать `NATS` или `Qdrant` authoritative state store;
- жёстко завязывать artifact plane только на одну реализацию вместо S3 API.

## 6. Git-дисциплина

Каждая отдельная semantic group в этом проекте должна коммититься отдельно.
Запрещено смешивать:
- docs/governance;
- compose/bootstrap;
- schema;
- Rust CLI/indexer;
- local runtime state.

Каталоги `state/**` и `tmp/**` не должны попадать в git.

## 7. Канонический runnable path

Минимальный runnable порядок для нового агента:
1. `scripts/bootstrap_stack.sh`
2. `scripts/status.sh`
3. `cargo run -- compat check`
4. `cargo run -- project register ...`
5. `cargo run -- namespace ensure ...`
6. `cargo run -- index project ...`
7. `cargo run -- context pack ...`

Для жёсткого локального proof:
- `scripts/proof_local.sh`
- `scripts/proof_hardening.sh`
- `scripts/proof_performance.sh`
- `scripts/proof_accuracy.sh`
- `scripts/proof_load.sh`
- `scripts/proof_hostile.sh`
- `scripts/proof_observability.sh`

Rust-native verification commands:
- `cargo run -- verify benchmark ...`
- `cargo run -- verify accuracy ...`
- `cargo run -- verify load ...`
- `cargo run -- verify hostile ...`
- `cargo run -- observe snapshot`
- `cargo run -- observe sla-check`

Если новый агент собирается утверждать, что проект «быстрый» или «устойчивый», он не имеет права
опираться только на `proof_local` и `proof_hardening`.
Минимально честный контур теперь включает:
- latency proof;
- accuracy/leakage proof;
- concurrent load proof;
- hostile fail-closed proof;
- recovery proof после возврата сервиса.
- observability snapshot;
- SLA check по machine-readable профилю.
