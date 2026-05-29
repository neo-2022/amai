#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

seed_file="${repo_root}/config/cold_repo_pool_seed.tsv"
pool_dir="${repo_root}/state/cold-benchmark/repo-pool"
manifest_path="${repo_root}/state/cold-benchmark/generated_manifest.toml"
target_repo_count=75
target_query_slice_count=225

while [[ $# -gt 0 ]]; do
    case "$1" in
        --seed)
            seed_file="$2"
            shift 2
            ;;
        --pool-dir)
            pool_dir="$2"
            shift 2
            ;;
        --manifest)
            manifest_path="$2"
            shift 2
            ;;
        --target-repo-count)
            target_repo_count="$2"
            shift 2
            ;;
        --target-query-slice-count)
            target_query_slice_count="$2"
            shift 2
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

mkdir -p "${pool_dir}"
mkdir -p "$(dirname "${manifest_path}")"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

manifest_repos="${tmp_dir}/repos.toml"
manifest_cases="${tmp_dir}/cases.toml"
repo_summary="${tmp_dir}/repo_summary.tsv"
skip_summary="${tmp_dir}/skip_summary.tsv"
: > "${manifest_repos}"
: > "${manifest_cases}"
printf "code\tdisplay_name\trepo_root\trepo_type\tsize_class\tquery_count\n" > "${repo_summary}"
printf "code\tsource_type\tlocation\treason\n" > "${skip_summary}"

readarray -t seed_rows < <(tail -n +2 "${seed_file}" | sed '/^[[:space:]]*$/d')

trim() {
    local value="$1"
    value="${value#"${value%%[![:space:]]*}"}"
    value="${value%"${value##*[![:space:]]}"}"
    printf '%s' "${value}"
}

toml_escape() {
    local value="$1"
    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    printf '%s' "${value}"
}

display_name_from_code() {
    local code="$1"
    printf '%s' "${code}" | tr '_' ' '
}

infer_repo_type_from_list() {
    local list_file="$1"
    if grep -Eq '(^|/)Cargo\.toml$' "${list_file}"; then
        printf 'rust_repo'
    elif grep -Eq '(^|/)package\.json$' "${list_file}"; then
        printf 'node_repo'
    elif grep -Eq '(^|/)pyproject\.toml$|(^|/)setup\.py$|(^|/)requirements\.txt$' "${list_file}"; then
        printf 'python_repo'
    elif grep -Eq '(^|/)go\.mod$' "${list_file}"; then
        printf 'go_repo'
    elif grep -Eq '(^|/)pom\.xml$|(^|/)build\.gradle(\.kts)?$' "${list_file}"; then
        printf 'jvm_repo'
    else
        printf 'mixed'
    fi
}

size_class_from_count() {
    local count="$1"
    if (( count < 200 )); then
        printf 'small'
    elif (( count < 1000 )); then
        printf 'medium'
    elif (( count < 5000 )); then
        printf 'large'
    else
        printf 'very_large'
    fi
}

first_matching_path() {
    local list_file="$1"
    shift
    local patterns=("$@")
    while IFS= read -r candidate; do
        for pattern in "${patterns[@]}"; do
            if [[ "${candidate}" == ${pattern} ]]; then
                printf '%s\n' "${candidate}"
                return 0
            fi
        done
    done < "${list_file}"
    return 1
}

append_unique_candidate() {
    local array_name="$1"
    local value="$2"
    [[ -z "${value}" ]] && return 0
    local -n target_ref="${array_name}"
    local item=""
    for item in "${target_ref[@]-}"; do
        if [[ "${item}" == "${value}" ]]; then
            return 0
        fi
    done
    target_ref+=("${value}")
}

select_candidates() {
    local list_file="$1"
    local -a selected=()

    local readme=""
    readme="$(first_matching_path "${list_file}" \
        "README.md" "README" "README.rst" "README.txt" \
        "readme.md" "Readme.md" "docs/README.md" "docs/index.md" || true)"
    append_unique_candidate selected "${readme}"

    local config=""
    config="$(first_matching_path "${list_file}" \
        "Cargo.toml" "package.json" "pyproject.toml" "go.mod" "pom.xml" \
        "build.gradle" "build.gradle.kts" "settings.gradle" "settings.gradle.kts" \
        "Makefile" "CMakeLists.txt" "WORKSPACE" "MODULE.bazel" "composer.json" \
        "mix.exs" "requirements.txt" "setup.py" "Gemfile" || true)"
    append_unique_candidate selected "${config}"

    local workflow=""
    workflow="$(first_matching_path "${list_file}" \
        "CONTRIBUTING.md" "Dockerfile" "justfile" "Taskfile.yml" "Taskfile.yaml" \
        "scripts/*.sh" "docs/installation.md" "docs/getting-started.md" || true)"
    append_unique_candidate selected "${workflow}"

    if (( ${#selected[@]} < 3 )); then
        while IFS= read -r candidate; do
            append_unique_candidate selected "${candidate}"
            if (( ${#selected[@]} >= 3 )); then
                break
            fi
        done < <(grep -E '(^|/)(README|LICENSE|CHANGELOG|CONTRIBUTING|Cargo\.toml|package\.json|pyproject\.toml|go\.mod|pom\.xml|build\.gradle(\.kts)?|Makefile|CMakeLists\.txt|Dockerfile|.*\.md|.*\.toml|.*\.json|.*\.ya?ml|.*\.sh)$' "${list_file}")
    fi

    printf '%s\n' "${selected[@]}"
}

append_cases_for_candidates() {
    local prefix="$1"
    shift
    local -a selected=("$@")
    local idx=0
    local selected_path=""
    for selected_path in "${selected[@]}"; do
        local slice="docs_lookup"
        if [[ "${selected_path}" == *.toml || "${selected_path}" == *.json || "${selected_path}" == *.yaml || "${selected_path}" == *.yml || "${selected_path}" == "go.mod" || "${selected_path}" == "Makefile" || "${selected_path}" == "pom.xml" || "${selected_path}" == "build.gradle" || "${selected_path}" == "build.gradle.kts" || "${selected_path}" == "CMakeLists.txt" || "${selected_path}" == "WORKSPACE" || "${selected_path}" == "MODULE.bazel" || "${selected_path}" == "requirements.txt" || "${selected_path}" == "setup.py" || "${selected_path}" == "Gemfile" || "${selected_path}" == "composer.json" || "${selected_path}" == "mix.exs" ]]; then
            slice="config_lookup"
        elif [[ "${selected_path}" == .github/workflows/* || "${selected_path}" == scripts/* || "${selected_path}" == "Dockerfile" || "${selected_path}" == "CONTRIBUTING.md" ]]; then
            slice="onboarding_query"
        fi
        cat >> "${manifest_cases}" <<EOF
[[cases]]
repo_code = "$(toml_escape "${prefix}")"
query_slice = "${slice}"
query = "$(toml_escape "${selected_path}")"
expected_projects = ["$(toml_escape "${prefix}")"]
expected_paths = ["$(toml_escape "${selected_path}")"]
limit_documents = 1
limit_symbols = 0
limit_chunks = 0
limit_semantic_chunks = 0

EOF
        idx=$((idx + 1))
    done
    printf '%s\n' "${idx}"
}

validate_manifest_expected_paths() {
    local manifest_file="$1"
    python3 - "${manifest_file}" <<'PY'
import sys
from pathlib import Path
import tomllib

manifest_path = Path(sys.argv[1])
data = tomllib.loads(manifest_path.read_text())

repo_roots = {}
for repo in data.get("repos", []):
    repo_roots[repo["code"]] = Path(repo["repo_root"])

missing = []
for case in data.get("cases", []):
    repo_code = case.get("repo_code")
    repo_root = repo_roots.get(repo_code)
    if repo_root is None:
        missing.append((repo_code or "<missing_repo>", "<unknown>", "repo_root_missing"))
        continue
    for rel_path in case.get("expected_paths", []):
        candidate = repo_root / rel_path
        if not candidate.exists():
            missing.append((repo_code, rel_path, str(candidate)))

if missing:
    print("manifest expected_paths drift detected:", file=sys.stderr)
    for repo_code, rel_path, candidate in missing:
        print(f"  - {repo_code}: {rel_path} -> {candidate}", file=sys.stderr)
    sys.exit(1)
PY
}

materialize_git_repo() {
    local code="$1"
    local url="$2"
    local checkout_dir="${pool_dir}/${code}"
    rm -rf "${checkout_dir}"
    local clone_attempt=1
    local clone_ok=0
    while (( clone_attempt <= 3 )); do
        if git clone --quiet --depth 1 --filter=blob:none --no-checkout "${url}" "${checkout_dir}" >/dev/null 2>&1; then
            clone_ok=1
            break
        fi
        rm -rf "${checkout_dir}"
        clone_attempt=$((clone_attempt + 1))
        sleep 2
    done
    if (( clone_ok == 0 )); then
        echo "warning: failed to clone ${code} from ${url} after 3 attempts" >&2
        return 1
    fi
    local tree_list="${tmp_dir}/${code}.tree"
    git -C "${checkout_dir}" ls-tree -r --name-only HEAD | sed '/^[[:space:]]*$/d' > "${tree_list}"
    local total_files
    total_files="$(wc -l < "${tree_list}")"
    local size_class
    size_class="$(size_class_from_count "${total_files}")"
    local repo_type
    repo_type="$(infer_repo_type_from_list "${tree_list}")"

    mapfile -t selected < <(select_candidates "${tree_list}")
    if (( ${#selected[@]} == 0 )); then
        echo "warning: no candidate files found for ${code}" >&2
        rm -rf "${checkout_dir}"
        return 1
    fi

    local sparse_file="${checkout_dir}/.git/info/sparse-checkout"
    git -C "${checkout_dir}" sparse-checkout init --no-cone >/dev/null 2>&1
    : > "${sparse_file}"
    local selected_path=""
    for selected_path in "${selected[@]}"; do
        printf '%s\n' "${selected_path}" >> "${sparse_file}"
    done
    if ! git -C "${checkout_dir}" checkout --quiet HEAD >/dev/null 2>&1; then
        echo "warning: failed to sparse-checkout ${code}" >&2
        rm -rf "${checkout_dir}"
        return 1
    fi

    local -a materialized=()
    for selected_path in "${selected[@]}"; do
        if [[ -f "${checkout_dir}/${selected_path}" ]]; then
            materialized+=("${selected_path}")
        else
            echo "warning: failed to materialize ${code}:${selected_path}" >&2
        fi
    done
    if (( ${#materialized[@]} == 0 )); then
        rm -rf "${checkout_dir}"
        return 1
    fi

    local display_name
    display_name="$(display_name_from_code "${code}")"
    cat >> "${manifest_repos}" <<EOF
[[repos]]
code = "$(toml_escape "${code}")"
display_name = "$(toml_escape "${display_name}")"
repo_root = "$(toml_escape "${checkout_dir}")"
namespace = "cold_benchmark"
repo_type = "${repo_type}"
size_class = "${size_class}"
limit_files = 16
skip_embeddings = false
default_retrieval_mode = "local_strict"

EOF
    local query_count
    query_count="$(append_cases_for_candidates "${code}" "${materialized[@]}")"
    printf "%s\t%s\t%s\t%s\t%s\t%s\n" \
        "${code}" "${display_name}" "${checkout_dir}" "${repo_type}" "${size_class}" "${query_count}" >> "${repo_summary}"
}

materialize_local_repo() {
    local code="$1"
    local raw_path="$2"
    local resolved_root
    if [[ "${raw_path}" = /* ]]; then
        resolved_root="${raw_path}"
    else
        resolved_root="$(cd "${repo_root}" && cd "${raw_path}" && pwd)"
    fi
    local list_file="${tmp_dir}/${code}.tree"
    if git -C "${resolved_root}" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        git -C "${resolved_root}" ls-files | sed '/^[[:space:]]*$/d' > "${list_file}"
    else
        find "${resolved_root}" -type f -printf '%P\n' | sed '/^[[:space:]]*$/d' > "${list_file}"
    fi
    local total_files
    total_files="$(wc -l < "${list_file}")"
    local size_class
    size_class="$(size_class_from_count "${total_files}")"
    local repo_type
    repo_type="$(infer_repo_type_from_list "${list_file}")"
    local display_name
    display_name="$(display_name_from_code "${code}")"
    local checkout_dir="${pool_dir}/${code}"
    rm -rf "${checkout_dir}"
    mkdir -p "${checkout_dir}"
    mapfile -t selected < <(select_candidates "${list_file}")
    if (( ${#selected[@]} == 0 )); then
        echo "warning: no candidate files found for local repo ${code}" >&2
        rm -rf "${checkout_dir}"
        return 1
    fi
    local selected_path=""
    for selected_path in "${selected[@]}"; do
        mkdir -p "${checkout_dir}/$(dirname "${selected_path}")"
        cp "${resolved_root}/${selected_path}" "${checkout_dir}/${selected_path}"
    done

    cat >> "${manifest_repos}" <<EOF
[[repos]]
code = "$(toml_escape "${code}")"
display_name = "$(toml_escape "${display_name}")"
repo_root = "$(toml_escape "${checkout_dir}")"
namespace = "cold_benchmark"
repo_type = "${repo_type}"
size_class = "${size_class}"
limit_files = 24
skip_embeddings = false
default_retrieval_mode = "local_strict"

EOF
    local query_count
    query_count="$(append_cases_for_candidates "${code}" "${selected[@]}")"
    printf "%s\t%s\t%s\t%s\t%s\t%s\n" \
        "${code}" "${display_name}" "${checkout_dir}" "${repo_type}" "${size_class}" "${query_count}" >> "${repo_summary}"
}

for row in "${seed_rows[@]}"; do
    IFS=$'\t' read -r code source_type location <<< "${row}"
    code="$(trim "${code}")"
    source_type="$(trim "${source_type}")"
    location="$(trim "${location}")"
    [[ -z "${code}" ]] && continue
    case "${source_type}" in
        local)
            if ! materialize_local_repo "${code}" "${location}"; then
                printf "%s\t%s\t%s\t%s\n" "${code}" "${source_type}" "${location}" "materialize_failed" >> "${skip_summary}"
            fi
            ;;
        git)
            if ! materialize_git_repo "${code}" "${location}"; then
                printf "%s\t%s\t%s\t%s\n" "${code}" "${source_type}" "${location}" "clone_or_sparse_failed" >> "${skip_summary}"
            fi
            ;;
        *)
            echo "unknown seed type ${source_type} for ${code}" >&2
            exit 1
            ;;
    esac
    current_repo_count="$(( $(wc -l < "${repo_summary}") - 1 ))"
    current_query_slice_count="$(grep -c '^\[\[cases\]\]' "${manifest_cases}" || true)"
    if (( current_repo_count >= target_repo_count && current_query_slice_count >= target_query_slice_count )); then
        break
    fi
done

repo_count="$(( $(wc -l < "${repo_summary}") - 1 ))"
query_slice_count="$(grep -c '^\[\[cases\]\]' "${manifest_cases}" || true)"

cat > "${manifest_path}" <<EOF
[profile]
display_name = "Large Real-Repos Cold Contour"
summary = "Честный end-to-end cold benchmark на большом реальном repo-pool: first-path retrieval без подмены component metrics."
target_p50_ms = 2.0
target_p95_ms = 5.0
target_p99_ms = 10.0
target_max_ms = 15.0
min_precision = 0.997
min_target_hit_rate = 0.997
min_recall = 0.997
min_sample_count = 1000
min_repo_count = 75
min_query_slice_count = 200
max_duration_seconds = 10.0
max_leakage = 0
max_error_rate = 0.0

EOF

cat "${manifest_repos}" >> "${manifest_path}"
cat "${manifest_cases}" >> "${manifest_path}"
validate_manifest_expected_paths "${manifest_path}"

echo "generated cold repo-pool manifest: ${manifest_path}"
echo "repo_count=${repo_count}"
echo "query_slice_count=${query_slice_count}"
echo "repo_summary=${repo_summary}"
echo "skip_summary=${skip_summary}"
