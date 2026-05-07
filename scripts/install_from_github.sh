#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: install_from_github.sh --repo-url <git-url> [options] [-- <install_amai args...>]

Clones or updates the Amai repository and then runs scripts/install_amai.sh from that clone.

Options:
  --repo-url <git-url>     Git URL to clone from. Required unless AMAI_GIT_REPO_URL is set.
  --clone-dir <path>       Where to place the clone. Default: $HOME/.local/share/amai/repo
  --repo-ref <ref>         Optional branch/tag/commit to check out before install.
  --help                   Show this help.

All remaining arguments are passed through to scripts/install_amai.sh inside the cloned repo.
EOF
}

repo_url="${AMAI_GIT_REPO_URL:-}"
clone_dir="${AMAI_GITHUB_CLONE_DIR:-${HOME}/.local/share/amai/repo}"
repo_ref=""
install_args=()
local_source_path=""

normalize_repo_url() {
  local value="$1"
  if [[ -d "${value}" ]]; then
    local abs
    abs="$(cd "${value}" && pwd)"
    printf 'file://%s\n' "${abs}"
    return
  fi
  printf '%s\n' "${value}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-url)
      repo_url="${2:?missing value for --repo-url}"
      shift 2
      ;;
    --repo-url=*)
      repo_url="${1#*=}"
      shift
      ;;
    --clone-dir)
      clone_dir="${2:?missing value for --clone-dir}"
      shift 2
      ;;
    --clone-dir=*)
      clone_dir="${1#*=}"
      shift
      ;;
    --repo-ref)
      repo_ref="${2:?missing value for --repo-ref}"
      shift 2
      ;;
    --repo-ref=*)
      repo_ref="${1#*=}"
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    --)
      shift
      install_args+=("$@")
      break
      ;;
    *)
      install_args+=("$1")
      shift
      ;;
  esac
done

if [[ -z "${repo_url}" ]]; then
  echo "install_from_github.sh requires --repo-url or AMAI_GIT_REPO_URL" >&2
  exit 64
fi

repo_url="$(normalize_repo_url "${repo_url}")"
if [[ "${repo_url}" == file://* ]]; then
  local_source_path="${repo_url#file://}"
fi

repair_local_source_checkout() {
  local source_root="$1"
  local clone_root="$2"
  local source_vendor="${source_root}/vendor"
  [[ -d "${source_vendor}" ]] || return
  mkdir -p "${clone_root}/vendor"
  cp -a "${source_vendor}/." "${clone_root}/vendor/"
}

assert_checkout_complete() {
  local clone_root="$1"
  if command -v cargo >/dev/null 2>&1; then
    if (cd "${clone_root}" && cargo metadata --offline --format-version 1 >/dev/null 2>&1); then
      return
    fi
  fi
  echo "install_from_github.sh: checkout is missing vendored source files required for cargo metadata." >&2
  echo "install_from_github.sh: refresh the published git source so vendor/ stays self-contained in the checkout." >&2
  exit 68
}

if ! command -v git >/dev/null 2>&1; then
  echo "install_from_github.sh requires git in PATH" >&2
  exit 127
fi

mkdir -p "$(dirname "${clone_dir}")"

if [[ ! -d "${clone_dir}" ]]; then
  if [[ -n "${repo_ref}" ]]; then
    git clone --depth 1 --branch "${repo_ref}" "${repo_url}" "${clone_dir}"
  else
    git clone --depth 1 "${repo_url}" "${clone_dir}"
  fi
elif [[ ! -d "${clone_dir}/.git" ]]; then
  echo "install_from_github.sh expected ${clone_dir} to be a git checkout" >&2
  exit 65
else
  git -C "${clone_dir}" remote set-url origin "${repo_url}"
  git -C "${clone_dir}" fetch --depth 1 origin
  if [[ -n "${repo_ref}" ]]; then
    git -C "${clone_dir}" checkout --force "${repo_ref}"
    git -C "${clone_dir}" reset --hard "origin/${repo_ref}" >/dev/null 2>&1 || true
  else
    current_branch="$(git -C "${clone_dir}" symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
    if [[ -n "${current_branch}" ]]; then
      git -C "${clone_dir}" pull --ff-only origin "${current_branch}"
    else
      default_ref="$(git -C "${clone_dir}" symbolic-ref --quiet refs/remotes/origin/HEAD 2>/dev/null || true)"
      default_branch="${default_ref#refs/remotes/origin/}"
      if [[ -n "${default_branch}" ]]; then
        git -C "${clone_dir}" checkout --force "${default_branch}"
        git -C "${clone_dir}" pull --ff-only origin "${default_branch}"
      else
        echo "install_from_github.sh could not determine the default branch for ${clone_dir}" >&2
        exit 66
      fi
    fi
  fi
fi

if [[ -n "${local_source_path}" ]]; then
  repair_local_source_checkout "${local_source_path}" "${clone_dir}"
fi

assert_checkout_complete "${clone_dir}"

if [[ ! -x "${clone_dir}/scripts/install_amai.sh" ]]; then
  echo "install_from_github.sh: ${clone_dir}/scripts/install_amai.sh is missing or not executable" >&2
  exit 67
fi

cd "${clone_dir}"
exec ./scripts/install_amai.sh "${install_args[@]}"
