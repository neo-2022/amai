modified_at: 2026-03-20 18:30 MSK
Ручная сверка guide/docs: 2026-03-20 18:30 MSK

# Tests

Этот каталог удерживает тестовый contour `Amai`.

Сейчас baseline такой:
- unit tests живут рядом с Rust modules;
- integration/smoke проверки выполняются через runtime commands из `docs/OPERATIONS.md`;
- локальный proof-cycle теперь состоит из четырёх слоёв:

```bash
./scripts/proof_local.sh
./scripts/proof_hardening.sh
./scripts/proof_performance.sh
./scripts/proof_mcp.sh
./scripts/proof_hostile.sh
```

Отдельно materialized Rust-native verification commands:
- `cargo run -- verify benchmark ...`
- `cargo run -- verify mcp ...`
- `cargo run -- verify hostile ...`

Когда появятся отдельные integration tests с поднимаемым stack fixture, они materialize-ятся именно здесь.
