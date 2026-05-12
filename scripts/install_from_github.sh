#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: install_from_github.sh [options] [-- <install_amai args...>]

Clones or updates the Amai repository and then runs scripts/install_amai.sh from that clone.

Options:
  --repo-url <git-url>     Git URL to clone from. Default: https://github.com/neo-2022/amai.git
  --clone-dir <path>       Where to place the clone. Default: $HOME/.local/share/amai/repo
  --repo-ref <ref>         Optional branch/tag/commit to check out before install.
  --download-mode <mode>   Checkout mode: auto|git|tarball. Default: auto.
  --help                   Show this help.

All remaining arguments are passed through to scripts/install_amai.sh inside the cloned repo.
EOF
}

default_public_repo_url="https://github.com/neo-2022/amai.git"
repo_url="${AMAI_GIT_REPO_URL:-${default_public_repo_url}}"
clone_dir="${AMAI_GITHUB_CLONE_DIR:-${HOME}/.local/share/amai/repo}"
repo_ref=""
download_mode="${AMAI_DOWNLOAD_MODE:-auto}"
install_args=()
local_source_path=""
is_git_checkout=0

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
    --download-mode)
      download_mode="${2:?missing value for --download-mode}"
      shift 2
      ;;
    --download-mode=*)
      download_mode="${1#*=}"
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

repo_url="$(normalize_repo_url "${repo_url}")"
if [[ "${repo_url}" == file://* ]]; then
  local_source_path="${repo_url#file://}"
fi

repair_local_source_checkout() {
  local source_root="$1"
  local clone_root="$2"
  local entry=""
  shopt -s dotglob nullglob
  for entry in "${source_root}"/*; do
    local base
    base="$(basename "${entry}")"
    case "${base}" in
      .git|target|state|tmp)
        continue
        ;;
    esac
    rm -rf "${clone_root:?}/${base}"
    cp -a "${entry}" "${clone_root}/${base}"
  done
  shopt -u dotglob nullglob
}

load_os_release() {
  [[ -f /etc/os-release ]] || return 1
  # shellcheck disable=SC1091
  source /etc/os-release
}

verified_auto_prereq_host() {
  load_os_release || return 1
  [[ "${AMAI_AUTO_INSTALL_PREREQS:-1}" != "0" ]] || return 1
  [[ "${ID:-}" == "ubuntu" || "${ID:-}" == "debian" || " ${ID_LIKE:-} " == *" debian "* ]]
}

run_as_root() {
  if [[ "${EUID}" -eq 0 ]]; then
    "$@"
    return
  fi
  if command -v sudo >/dev/null 2>&1; then
    sudo "$@"
    return
  fi
  echo "install_from_github.sh automatic prerequisite bootstrap requires root or sudo on verified Ubuntu/Debian hosts." >&2
  return 1
}

ensure_git_for_verified_host() {
  verified_auto_prereq_host || return 0
  command -v git >/dev/null 2>&1 && return 0
  export DEBIAN_FRONTEND=noninteractive
  run_as_root apt-get update
  run_as_root apt-get install -y git curl ca-certificates
}

ensure_tarball_tools_for_verified_host() {
  verified_auto_prereq_host || return 0
  local missing=()
  command -v curl >/dev/null 2>&1 || missing+=(curl)
  command -v tar >/dev/null 2>&1 || missing+=(tar)
  command -v ca-certificates >/dev/null 2>&1 || true
  if [[ "${#missing[@]}" -eq 0 ]]; then
    return 0
  fi
  export DEBIAN_FRONTEND=noninteractive
  run_as_root apt-get update
  run_as_root apt-get install -y curl ca-certificates tar
}

github_tarball_url() {
  local url="$1"
  local ref="$2"
  local normalized="${url%.git}"
  if [[ "${normalized}" != https://github.com/*/* ]]; then
    return 1
  fi
  local path="${normalized#https://github.com/}"
  local owner="${path%%/*}"
  local repo="${path#*/}"
  [[ -n "${owner}" && -n "${repo}" ]] || return 1
  local tar_ref="${ref}"
  tar_ref="${tar_ref#refs/heads/}"
  tar_ref="${tar_ref#refs/tags/}"
  if [[ -z "${tar_ref}" ]]; then
    printf 'https://codeload.github.com/%s/%s/tar.gz/refs/heads/main\n' "${owner}" "${repo}"
    printf 'https://codeload.github.com/%s/%s/tar.gz/refs/heads/master\n' "${owner}" "${repo}"
    return 0
  fi
  printf 'https://codeload.github.com/%s/%s/tar.gz/%s\n' "${owner}" "${repo}" "${tar_ref}"
}

checkout_from_github_tarball() {
  local url="$1"
  local ref="$2"
  local out_dir="$3"

  local tar_urls=()
  local line=""
  while IFS= read -r line; do
    [[ -n "${line}" ]] && tar_urls+=("${line}")
  done < <(github_tarball_url "${url}" "${ref}") || return 1

  ensure_tarball_tools_for_verified_host || true
  command -v curl >/dev/null 2>&1 || {
    echo "install_from_github.sh: curl is required for tarball checkout (missing curl in PATH)." >&2
    return 127
  }
  command -v tar >/dev/null 2>&1 || {
    echo "install_from_github.sh: tar is required for tarball checkout (missing tar in PATH)." >&2
    return 127
  }

  local tmp
  tmp="$(mktemp -d)"
  trap "rm -rf '${tmp}'" RETURN

  local tar_url=""
  local ok=0
  for tar_url in "${tar_urls[@]}"; do
    echo "install_from_github.sh: downloading tarball from ${tar_url}" >&2
    if curl -fL --retry 3 --retry-delay 1 --retry-all-errors -o "${tmp}/repo.tar.gz" "${tar_url}"; then
      ok=1
      break
    fi
  done
  if [[ "${ok}" -ne 1 ]]; then
    echo "install_from_github.sh: failed to download a GitHub tarball from any candidate URL." >&2
    return 69
  fi
  tar -xzf "${tmp}/repo.tar.gz" -C "${tmp}"

  local root
  root="$(find "${tmp}" -mindepth 1 -maxdepth 1 -type d -printf '%p\n' | head -n 1)"
  if [[ -z "${root}" || ! -d "${root}" ]]; then
    echo "install_from_github.sh: failed to locate extracted tarball directory under ${tmp}" >&2
    return 65
  fi

  rm -rf "${out_dir}"
  mkdir -p "$(dirname "${out_dir}")"
  mv "${root}" "${out_dir}"
}

assert_checkout_complete() {
  local clone_root="$1"
  local cargo_bin=""
  local rustc_bin=""
  local metadata_stderr
  metadata_stderr="$(mktemp)"
  trap 'rm -f "${metadata_stderr}"' RETURN
  if [[ -x "${clone_root}/scripts/resolve_cargo.sh" && -x "${clone_root}/scripts/resolve_rustc.sh" ]]; then
    cargo_bin="$("${clone_root}/scripts/resolve_cargo.sh")" || {
      echo "install_from_github.sh: unable to materialize a working cargo toolchain for ${clone_root}." >&2
      echo "install_from_github.sh: repair rustup/cargo availability or publish a bootstrap that installs Rust automatically." >&2
      exit 68
    }
    rustc_bin="$("${clone_root}/scripts/resolve_rustc.sh")" || {
      echo "install_from_github.sh: unable to materialize a working rustc toolchain for ${clone_root}." >&2
      echo "install_from_github.sh: repair rustup/rustc availability or publish a bootstrap that installs Rust automatically." >&2
      exit 68
    }
    if (cd "${clone_root}" && env RUSTC="${rustc_bin}" "${cargo_bin}" metadata --offline --format-version 1 >/dev/null 2>"${metadata_stderr}"); then
      return
    fi
  elif command -v cargo >/dev/null 2>&1; then
    if (cd "${clone_root}" && cargo metadata --offline --format-version 1 >/dev/null 2>"${metadata_stderr}"); then
      return
    fi
  fi
  if grep -Eq 'failed to calculate checksum of|No such file or directory.*vendor|can.t find.*vendor|failed to load manifest for dependency' "${metadata_stderr}"; then
    echo "install_from_github.sh: checkout is missing vendored source files required for cargo metadata." >&2
    echo "install_from_github.sh: refresh the published git source so vendor/ stays self-contained in the checkout." >&2
    sed -n '1,40p' "${metadata_stderr}" >&2
    exit 68
  fi
  echo "install_from_github.sh: cargo metadata failed before install bootstrap could continue." >&2
  echo "install_from_github.sh: this machine still has a broken Rust toolchain or another cargo-level failure." >&2
  sed -n '1,40p' "${metadata_stderr}" >&2
  exit 68
}

case "${download_mode}" in
  auto|git|tarball) ;;
  *)
    echo "install_from_github.sh: unknown --download-mode value: ${download_mode} (expected: auto|git|tarball)" >&2
    exit 64
    ;;
esac

mkdir -p "$(dirname "${clone_dir}")"

if [[ ! -d "${clone_dir}" ]]; then
  if [[ "${download_mode}" == "tarball" ]]; then
    checkout_from_github_tarball "${repo_url}" "${repo_ref}" "${clone_dir}"
    is_git_checkout=0
  else
    ensure_git_for_verified_host
    if ! command -v git >/dev/null 2>&1; then
      if [[ "${download_mode}" == "git" ]]; then
        echo "install_from_github.sh requires git in PATH" >&2
        exit 127
      fi
      checkout_from_github_tarball "${repo_url}" "${repo_ref}" "${clone_dir}"
      is_git_checkout=0
    else
      if [[ -n "${repo_ref}" ]]; then
        git clone --depth 1 --branch "${repo_ref}" "${repo_url}" "${clone_dir}" || {
          [[ "${download_mode}" == "git" ]] && exit 69
          checkout_from_github_tarball "${repo_url}" "${repo_ref}" "${clone_dir}"
        }
      else
        git clone --depth 1 "${repo_url}" "${clone_dir}" || {
          [[ "${download_mode}" == "git" ]] && exit 69
          checkout_from_github_tarball "${repo_url}" "${repo_ref}" "${clone_dir}"
        }
      fi
      is_git_checkout=1
    fi
  fi
elif [[ ! -d "${clone_dir}/.git" ]]; then
  if [[ "${download_mode}" == "git" ]]; then
    echo "install_from_github.sh expected ${clone_dir} to be a git checkout" >&2
    exit 65
  fi
  checkout_from_github_tarball "${repo_url}" "${repo_ref}" "${clone_dir}"
  is_git_checkout=0
else
  git -C "${clone_dir}" remote set-url origin "${repo_url}"
  if [[ "${download_mode}" == "tarball" ]]; then
    checkout_from_github_tarball "${repo_url}" "${repo_ref}" "${clone_dir}"
    is_git_checkout=0
  else
    ensure_git_for_verified_host
    if ! git -C "${clone_dir}" fetch --depth 1 origin; then
      [[ "${download_mode}" == "git" ]] && exit 69
      checkout_from_github_tarball "${repo_url}" "${repo_ref}" "${clone_dir}"
      is_git_checkout=0
    else
      is_git_checkout=1
    fi
  fi
  if [[ "${is_git_checkout}" -eq 1 ]]; then
    if [[ -n "${repo_ref}" ]]; then
      git -C "${clone_dir}" checkout --force "${repo_ref}"
      git -C "${clone_dir}" reset --hard "origin/${repo_ref}" >/dev/null 2>&1 || true
    else
      current_branch="$(git -C "${clone_dir}" symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
      if [[ -n "${current_branch}" ]]; then
        git -C "${clone_dir}" checkout --force "${current_branch}"
        git -C "${clone_dir}" reset --hard "origin/${current_branch}"
      else
        default_ref="$(git -C "${clone_dir}" symbolic-ref --quiet refs/remotes/origin/HEAD 2>/dev/null || true)"
        default_branch="${default_ref#refs/remotes/origin/}"
        if [[ -n "${default_branch}" ]]; then
          git -C "${clone_dir}" checkout --force "${default_branch}"
          git -C "${clone_dir}" reset --hard "origin/${default_branch}"
        else
          echo "install_from_github.sh could not determine the default branch for ${clone_dir}" >&2
          exit 66
        fi
      fi
    fi
  fi
fi

if [[ -n "${local_source_path}" ]]; then
  repair_local_source_checkout "${local_source_path}" "${clone_dir}"
fi

if [[ -f "${clone_dir}/scripts/ensure_verified_linux_prereqs.sh" ]]; then
  # shellcheck disable=SC1091
  source "${clone_dir}/scripts/ensure_verified_linux_prereqs.sh"
  ensure_verified_linux_prereqs 0
fi

assert_checkout_complete "${clone_dir}"

if [[ ! -x "${clone_dir}/scripts/install_amai.sh" ]]; then
  echo "install_from_github.sh: ${clone_dir}/scripts/install_amai.sh is missing or not executable" >&2
  exit 67
fi

cd "${clone_dir}"
exec ./scripts/install_amai.sh "${install_args[@]}"
