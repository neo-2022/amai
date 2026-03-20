modified_at: 2026-03-20 20:55 MSK
Ручная сверка guide/docs: 2026-03-20 20:55 MSK

# Tests

Этот каталог удерживает тестовый contour `Amai`.

Сейчас baseline такой:
- unit tests живут рядом с Rust modules;
- integration/smoke проверки выполняются через runtime commands из `docs/OPERATIONS.md`;
- локальный proof-cycle теперь состоит из нескольких отдельных контуров:

```bash
./scripts/proof_local.sh
./scripts/proof_hardening.sh
./scripts/proof_performance.sh
./scripts/proof_accuracy.sh
./scripts/proof_load.sh
./scripts/proof_stress_scale.sh
./scripts/proof_token_benchmark.sh
./scripts/proof_token_benchmark_suite.sh
./scripts/proof_observability.sh
./scripts/proof_mcp.sh
./scripts/proof_hostile.sh
./scripts/proof_text_compare.sh
```

Отдельно materialized Rust-native verification commands:
- `cargo run -- verify benchmark ...`
- `cargo run -- verify accuracy ...`
- `cargo run -- verify load ...`
- `cargo run -- verify token-benchmark ...`
- `cargo run -- verify token-benchmark-suite ...`
- `cargo run -- verify text-compare ...`
- `cargo run -- verify mcp ...`
- `cargo run -- verify hostile ...`

Когда появятся отдельные integration tests с поднимаемым stack fixture, они materialize-ятся именно здесь.
