modified_at: 2026-03-24 23:20 MSK
Ручная сверка guide/docs: 2026-03-24 23:20 MSK

# Deployment Targets

Этот документ нужен для простого вопроса:

`Как вообще можно развернуть Amai и какой путь выбирать именно мне?`

Самая важная мысль:
- `deployment profile`
  - это про силу машины;
- `deployment target`
  - это про сам способ развёртывания.

То есть сначала полезно понимать не только `хватит ли ресурсов`, но и `какой вообще режим мне нужен`.

Канонический список режимов хранится в:

```bash
config/deployment_targets.toml
```

## Быстрый список

Если хотите просто увидеть все режимы:

```bash
./scripts/deployment_targets.sh
```

Если хотите подробнее по одному режиму:

```bash
cargo run -- deployment explain --target local_docker
cargo run -- deployment explain --target kubernetes_server
```

Если хотите проверить, готова ли именно эта машина:

```bash
./scripts/deployment_preflight.sh --target local_docker
./scripts/deployment_preflight.sh --target windows_vm_lab
```

## `local_docker`

Это главный режим прямо сейчас.

Простыми словами:
- это обычный нормальный baseline для локальной машины;
- или для одного Linux-хоста;
- именно от него сейчас идёт основной product path.

Когда его выбирать:
- хотите просто честно поставить `Amai` и пользоваться;
- нужен основной локальный install path;
- нужен baseline для proof, observability и MCP.

Когда его не выбирать:
- если вы уже строите более серьёзный server/team layer;
- если вам нужен не локальный baseline, а следующий deployment contour.

## `remote_ssh`

Это режим, когда `Amai` живёт на Linux/VPS, а IDE у вас локально.

Простыми словами:
- сервер и базы живут рядом;
- локально у пользователя только клиент;
- клиент запускает удалённый `Amai` через `ssh`.

Почему это хорошо:
- не нужно выставлять внутренние базы в интернет;
- удобно для Windows/macOS-клиента;
- хорошо ложится на VPS и remote product path.

## `kubernetes_server`

Это не обязательный путь для обычного человека.

Простыми словами:
- это следующий deployment layer для команды или server-инфраструктуры;
- он нужен не для того, чтобы усложнить первую установку;
- он нужен там, где уже появляются более серьёзные серверные требования.

Когда он полезен:
- rolling updates;
- следующий шаг после одиночного Docker baseline;
- будущий рост вокруг auth/license/subscription и server orchestration.

Что важно не перепутать:
- сейчас `Kubernetes` не должен подменять обычный install path;
- текущий baseline по-прежнему `local_docker`.

## `windows_vm_lab`

Это не отдельный режим для повседневной работы.

Простыми словами:
- это лаборатория проверки;
- через неё теперь уже можно честно валидировать Windows-клиентский путь живым execute-runner-ом;
- это нужно, чтобы не обещать Windows-поддержку без реального proof.

Когда он нужен:
- нужно прогнать Windows smoke;
- нужно проверить install/remove/MCP в Windows-контуре;
- нужно зафиксировать future support не словами, а проверкой.
- текущий канонический доказанный сценарий внутри этого контура: локальный `install_amai.ps1` на Windows честно уходит в fail-closed и требует `WSL2` или `--ssh-destination`, а proof собирает это как evidence через VM.

## Как думать о режиме правильно

Самая полезная короткая схема такая:
- хотите обычную локальную установку:
  - `local_docker`
- хотите держать `Amai` на Linux/VPS и подключаться удалённо:
  - `remote_ssh`
- хотите следующий server/team layer:
  - `kubernetes_server`
- хотите честно валидировать Windows path:
  - `windows_vm_lab`

Живой вход в этот контур сейчас такой:

```bash
./scripts/deployment_preflight.sh --target windows_vm_lab
./scripts/proof_windows_vm_lab.sh --iso-path /path/to/windows.iso
```

## Следующий шаг

Если вы обычный пользователь, почти всегда правильный путь сейчас такой:

```bash
./scripts/install_amai.sh
```

Если вы инженер и хотите посмотреть deployment-режимы глубже:

```bash
./scripts/deployment_targets.sh
./scripts/deployment_preflight.sh --target local_docker
```
