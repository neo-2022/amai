#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

explicit_profile=0
stack_profile=""
install_args=()

args=("$@")
for ((i=0; i<${#args[@]}; i++)); do
  case "${args[$i]}" in
    --stack-profile)
      explicit_profile=1
      stack_profile="${args[$((i + 1))]:?missing value for --stack-profile}"
      i=$((i + 1))
      ;;
    --stack-profile=*)
      explicit_profile=1
      stack_profile="${args[$i]#*=}"
      ;;
    *)
      install_args+=("${args[$i]}")
      ;;
  esac
done

run_preflight() {
  cargo run --quiet -- bootstrap preflight --stack-profile "$1"
}

extract_line() {
  local label="$1"
  local output="$2"
  awk -v label="$label" '
    index($0, label) == 1 {
      print $0
      exit
    }
  ' <<<"$output"
}

extract_field() {
  local prefix="$1"
  local output="$2"
  awk -v prefix="$prefix" '
    index($0, prefix) == 1 {
      line = $0
      sub("^" prefix, "", line)
      print line
      exit
    }
  ' <<<"$output"
}

normalize_verdict() {
  case "$1" in
    "машина подходит")
      printf 'pass\n'
      ;;
    "машина подходит с оговорками")
      printf 'warn\n'
      ;;
    "машина не подходит для этого режима")
      printf 'fail\n'
      ;;
    *)
      printf 'unknown\n'
      ;;
  esac
}

verdict_short() {
  case "$1" in
    pass)
      printf 'подходит\n'
      ;;
    warn)
      printf 'подходит с оговорками\n'
      ;;
    fail)
      printf 'не подходит\n'
      ;;
    *)
      printf 'статус неясен\n'
      ;;
  esac
}

interactive_prompt_enabled() {
  if [[ "${AMAI_NO_INSTALL_PROMPT:-0}" == "1" ]]; then
    return 1
  fi
  if [[ "${AMAI_FORCE_INTERACTIVE_PROMPT:-0}" == "1" ]]; then
    return 0
  fi
  [[ -t 0 && -t 1 ]]
}

confirm_install() {
  local chosen_profile="$1"
  local chosen_label="$2"
  local chosen_verdict="$3"

  echo
  if [[ "$chosen_verdict" == "warn" ]]; then
    echo "ПРЕДУПРЕЖДЕНИЕ: профиль ${chosen_label} этой машине подходит, но без запаса."
    echo "Такой режим можно ставить, если вас устраивает более скромный запас по тяжёлым сценариям."
  fi

  if [[ "$chosen_verdict" == "fail" ]]; then
    echo "ПРЕДУПРЕЖДЕНИЕ: профиль ${chosen_label} этой машине не подходит."
    echo "Установка не начата. Выберите другой профиль или более сильную машину."
    return 0
  fi

  read -r -p "Напишите ДА, если хотите установить Amai в режиме ${chosen_label}: " answer
  case "$answer" in
    ДА|да|Да|YES|Yes|yes|Y|y)
      exec env \
        AMAI_PREFLIGHT_ALREADY_SHOWN=1 \
        AMAI_SKIP_STACK_PREFLIGHT=1 \
        ./scripts/install_amai.sh "${install_args[@]}" --stack-profile "$chosen_profile" --yes
      ;;
    *)
      echo "Установка не запущена. Когда захотите продолжить, снова запустите проверку."
      ;;
  esac
}

selector_mode="${AMAI_SELECTOR_MODE:-check}"

if [[ "$explicit_profile" -eq 1 ]]; then
  output="$(run_preflight "$stack_profile")"
  printf '%s\n' "$output"
  exit 0
fi

default_output="$(run_preflight default)"
lite_output="$(run_preflight lite_vps)"

default_label="$(extract_field 'Профиль: ' "$default_output")"
lite_label="$(extract_field 'Профиль: ' "$lite_output")"
default_verdict_title="$(extract_field 'Итог: ' "$default_output")"
lite_verdict_title="$(extract_field 'Итог: ' "$lite_output")"
default_verdict="$(normalize_verdict "$default_verdict_title")"
lite_verdict="$(normalize_verdict "$lite_verdict_title")"

echo "Amai preflight"
echo
echo "Эта команда сразу проверила два режима установки и покажет, что ваша машина реально тянет."
echo
echo "Что увидела машина:"
extract_line '- CPU:' "$default_output"
extract_line '- Память:' "$default_output"
extract_line '- Диск:' "$default_output"
echo
echo "Профили установки:"
echo "1. ${default_label} — $(verdict_short "$default_verdict")"
echo "2. ${lite_label} — $(verdict_short "$lite_verdict")"
echo

recommended_choice=""
recommended_reason=""
if [[ "$default_verdict" == "pass" ]]; then
  recommended_choice="1"
  recommended_reason="Это основной полноценный режим, и у этой машины для него есть хороший запас."
elif [[ "$default_verdict" == "warn" ]]; then
  recommended_choice="1"
  recommended_reason="Полноценный режим возможен, но уже без большого запаса. Если нужен более лёгкий вариант, можно выбрать 2."
elif [[ "$lite_verdict" == "pass" || "$lite_verdict" == "warn" ]]; then
  recommended_choice="2"
  recommended_reason="Полноценный локальный режим сейчас тяжёлый, зато лёгкий удалённый режим машина тянет."
fi

if [[ -n "$recommended_choice" ]]; then
  echo "Рекомендуемый выбор:"
  if [[ "$recommended_choice" == "1" ]]; then
    echo "- 1. ${default_label}"
  else
    echo "- 2. ${lite_label}"
  fi
  echo "- ${recommended_reason}"
else
  echo "Рекомендуемый выбор:"
  echo "- Сейчас нет профиля, который эта машина тянет без блокирующих ограничений."
fi

echo
echo "Если хотите только посмотреть результат, можно остановиться здесь."
if [[ "$selector_mode" == "install" ]]; then
  echo "Если хотите установить Amai, ниже можно выбрать профиль."
fi

if [[ "$selector_mode" != "install" ]]; then
  exit 0
fi

if ! interactive_prompt_enabled; then
  exit 0
fi

echo
printf 'Введите 1 или 2, чтобы начать установку. Нажмите Enter, если пока ставить не нужно: '
read -r choice

case "$choice" in
  1)
    confirm_install "default" "$default_label" "$default_verdict"
    ;;
  2)
    confirm_install "lite_vps" "$lite_label" "$lite_verdict"
    ;;
  "")
    echo "Установка не запущена. Вы просто посмотрели, что тянет машина."
    ;;
  *)
    echo "Непонятный выбор. Установка не запущена."
    ;;
esac
