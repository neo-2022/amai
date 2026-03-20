#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

list_output="$(./scripts/deployment_targets.sh)"
printf '%s\n' "$list_output" | rg '^1\. Local Docker Baseline \(local_docker\) — уже поддержано$' >/dev/null
printf '%s\n' "$list_output" | rg '^3\. Kubernetes Server Layer \(kubernetes_server\) — задел готов$' >/dev/null
printf '%s\n' "$list_output" | rg '^4\. Windows VM Validation Lab \(windows_vm_lab\) — задел готов$' >/dev/null

docker_output="$(./scripts/deployment_preflight.sh --target local_docker)"
printf '%s\n' "$docker_output" | rg '^Готовность этой машины: готово к работе$' >/dev/null
printf '%s\n' "$docker_output" | rg 'docker: найдено' >/dev/null

k8s_output="$(./scripts/deployment_preflight.sh --target kubernetes_server)"
printf '%s\n' "$k8s_output" | rg '^Статус в продукте: задел уже заложен$' >/dev/null
printf '%s\n' "$k8s_output" | rg 'kubectl: найдено' >/dev/null

windows_output="$(./scripts/deployment_preflight.sh --target windows_vm_lab)"
printf '%s\n' "$windows_output" | rg '^Статус в продукте: задел уже заложен$' >/dev/null
printf '%s\n' "$windows_output" | rg 'qemu-system-x86_64: найдено' >/dev/null
