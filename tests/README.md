modified_at: 2026-03-20 14:08 MSK
Ручная сверка guide/docs: 2026-03-20 14:08 MSK

# Tests

Этот каталог удерживает тестовый contour `Amai`.

Сейчас baseline такой:
- unit tests живут рядом с Rust modules;
- integration/smoke проверки выполняются через runtime commands из `docs/OPERATIONS.md`;
- локальный полный proof-cycle запускается через:

```bash
./scripts/proof_local.sh
```

Когда появятся отдельные integration tests с поднимаемым stack fixture, они materialize-ятся именно здесь.
