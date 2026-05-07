#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

state_dir="${tmpdir}/state"
bin_dir="${tmpdir}/bin"
mkdir -p "${state_dir}" "${bin_dir}"

cat >"${bin_dir}/rustup" <<EOF
#!/usr/bin/env bash
set -euo pipefail
state_dir="${state_dir}"
case "\$1" in
  show)
    if [[ "\${2:-}" == "active-toolchain" ]]; then
      [[ -f "\${state_dir}/active-toolchain" ]] || exit 1
      cat "\${state_dir}/active-toolchain"
      exit 0
    fi
    ;;
  default)
    [[ "\${2:-}" == "stable" ]] || exit 1
    printf 'stable-x86_64-unknown-linux-gnu\n' > "\${state_dir}/active-toolchain"
    exit 0
    ;;
esac
exit 1
EOF

cat >"${bin_dir}/cargo" <<EOF
#!/usr/bin/env bash
set -euo pipefail
state_dir="${state_dir}"
[[ -f "\${state_dir}/active-toolchain" ]] || exit 1
if [[ "\${1:-}" == "--version" ]]; then
  printf 'cargo 1.99.0 (proof stub)\n'
  exit 0
fi
exit 0
EOF

cat >"${bin_dir}/rustc" <<EOF
#!/usr/bin/env bash
set -euo pipefail
state_dir="${state_dir}"
[[ -f "\${state_dir}/active-toolchain" ]] || exit 1
if [[ "\${1:-}" == "-vV" ]]; then
  printf 'rustc 1.99.0 (proof stub)\n'
  exit 0
fi
exit 0
EOF

chmod +x "${bin_dir}/rustup" "${bin_dir}/cargo" "${bin_dir}/rustc"

resolved_cargo="$(
  PATH="${bin_dir}:${PATH}" ./scripts/resolve_cargo.sh
)"
[[ "${resolved_cargo}" == "${bin_dir}/cargo" ]]
[[ -f "${state_dir}/active-toolchain" ]]

rm -f "${state_dir}/active-toolchain"

resolved_rustc="$(
  PATH="${bin_dir}:${PATH}" ./scripts/resolve_rustc.sh
)"
[[ "${resolved_rustc}" == "${bin_dir}/rustc" ]]
[[ -f "${state_dir}/active-toolchain" ]]

printf 'proof_rustup_default_toolchain_autorepair: ok\n'
