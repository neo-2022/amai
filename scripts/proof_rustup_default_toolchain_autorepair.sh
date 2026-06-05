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
  toolchain)
    if [[ "\${2:-}" == "install" && "\${3:-}" == "stable" ]]; then
      printf 'usable\n' > "\${state_dir}/toolchain-usable"
      exit 0
    fi
    ;;
esac
exit 1
EOF

cat >"${bin_dir}/cargo" <<EOF
#!/usr/bin/env bash
set -euo pipefail
state_dir="${state_dir}"
[[ -f "\${state_dir}/active-toolchain" ]] || exit 1
[[ -f "\${state_dir}/toolchain-usable" ]] || exit 1
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
[[ -f "\${state_dir}/toolchain-usable" ]] || exit 1
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
[[ -f "${state_dir}/toolchain-usable" ]]

rm -f "${state_dir}/active-toolchain"
rm -f "${state_dir}/toolchain-usable"

resolved_rustc="$(
  PATH="${bin_dir}:${PATH}" ./scripts/resolve_rustc.sh
)"
[[ "${resolved_rustc}" == "${bin_dir}/rustc" ]]
[[ -f "${state_dir}/active-toolchain" ]]
[[ -f "${state_dir}/toolchain-usable" ]]

rm -f "${state_dir}/toolchain-usable"

resolved_cargo_broken_active="$(
  PATH="${bin_dir}:${PATH}" ./scripts/resolve_cargo.sh
)"
[[ "${resolved_cargo_broken_active}" == "${bin_dir}/cargo" ]]
[[ -f "${state_dir}/active-toolchain" ]]
[[ -f "${state_dir}/toolchain-usable" ]]

hang_bin_dir="${tmpdir}/hang-bin"
mkdir -p "${hang_bin_dir}"
cat >"${hang_bin_dir}/cargo" <<'EOF'
#!/usr/bin/env bash
sleep 60
EOF

cat >"${hang_bin_dir}/rustup" <<'EOF'
#!/usr/bin/env bash
sleep 60
EOF

chmod +x "${hang_bin_dir}/cargo" "${hang_bin_dir}/rustup"

set +e
PATH="${hang_bin_dir}:${PATH}" \
  AMAI_CARGO_CHECK_TIMEOUT_SECONDS=1 \
  AMAI_RUSTUP_REPAIR_TIMEOUT_SECONDS=1 \
  timeout 12s ./scripts/resolve_cargo.sh >"${tmpdir}/hanging-resolver.out" 2>"${tmpdir}/hanging-resolver.err"
hanging_status=$?
set -e
[[ "${hanging_status}" != "124" ]]

printf 'proof_rustup_default_toolchain_autorepair: ok\n'
