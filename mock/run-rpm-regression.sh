#!/usr/bin/env bash
# SPDX-License-Identifier: MulanPSL-2.0
set -uo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_dir=$(cd -- "$script_dir/.." && pwd)

config=${MOCK_CONFIG:-"$script_dir/example/openruyi-rpm-rolling-unstable-x86_64.cfg"}
if [[ -z ${MOCK_CONFIG:-} ]]; then
    config_tmp=$(mktemp "${TMPDIR:-/tmp}/mock-llvmir-regression.XXXXXX.cfg")
    trap 'rm -f -- "$config_tmp"' EXIT
    sed "s|/path/to/clang-wrap/mock|$script_dir|g" "$config" > "$config_tmp"
    config=$config_tmp
fi
srpm_dir=${SRPM_DIR:-"$script_dir/test-srpms"}
output_dir=${REGRESSION_OUTPUT_DIR:-"$repo_dir/mock-regression-results"}
mock_command=${MOCK_COMMAND:-mock}
packages=(sqlite libxml2 libwebp brotli nghttp3 wayland) # mariadb
selected_packages=()

usage() {
    cat <<'EOF'
Usage: run-rpm-regression.sh [options] [PACKAGE ...]

Build SRPMs with Mock and require at least one generated *-llvmir RPM per
package. Without PACKAGE arguments, the default matrix is:
  sqlite libxml2 libwebp brotli nghttp3 wayland

Options:
  -c, --config FILE       Mock config (default: mock/example/...cfg)
  -s, --srpm-dir DIR      Directory containing input *.src.rpm files
  -o, --output-dir DIR    Parent directory for timestamped run results
  -m, --mock COMMAND      Mock executable (default: mock)
  -h, --help              Show this help

Environment overrides: MOCK_CONFIG, SRPM_DIR, REGRESSION_OUTPUT_DIR,
MOCK_COMMAND.
EOF
}

while (($#)); do
    case "$1" in
        -c|--config) config=$2; shift 2 ;;
        -s|--srpm-dir) srpm_dir=$2; shift 2 ;;
        -o|--output-dir) output_dir=$2; shift 2 ;;
        -m|--mock) mock_command=$2; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        --) shift; selected_packages+=("$@"); break ;;
        -*) printf 'Unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
        *) selected_packages+=("$1"); shift ;;
    esac
done

if ((${#selected_packages[@]} == 0)); then
    selected_packages=("${packages[@]}")
fi

if [[ ! -f $config ]]; then
    printf 'Mock config not found: %s\n' "$config" >&2
    exit 2
fi
if [[ ! -d $srpm_dir ]]; then
    printf 'SRPM directory not found: %s\n' "$srpm_dir" >&2
    exit 2
fi
if ! command -v "$mock_command" >/dev/null 2>&1; then
    printf 'Mock command not found: %s\n' "$mock_command" >&2
    exit 2
fi

run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
run_dir="$output_dir/$run_id"
mkdir -p -- "$run_dir"
summary="$run_dir/summary.tsv"
printf 'package\tstatus\tsrpm\tllvmir_rpms\tlog\n' > "$summary"

failures=0
for package in "${selected_packages[@]}"; do
    matches=()
    case "$package" in
        libxml) package_glob=libxml2 ;;
        sqlite3) package_glob=sqlite ;;
        *) package_glob=$package ;;
    esac
    shopt -s nullglob
    matches=("$srpm_dir"/"$package_glob"-*.src.rpm)
    shopt -u nullglob

    if ((${#matches[@]} != 1)); then
        printf 'FAIL %-12s expected one %s-*.src.rpm in %s (found %d)\n' \
            "$package" "$package_glob" "$srpm_dir" "${#matches[@]}" >&2
        printf '%s\tFAIL\t%s\t0\t%s\n' "$package" "(missing or ambiguous SRPM)" "" >> "$summary"
        failures=$((failures + 1))
        continue
    fi

    srpm=${matches[0]}
    package_dir="$run_dir/$package"
    log="$package_dir/mock.log"
    mkdir -p -- "$package_dir"
    printf 'BUILD %-12s %s\n' "$package" "$srpm"

    if "$mock_command" -r "$config" --resultdir "$package_dir" --rebuild "$srpm" >"$log" 2>&1; then
        llvmir_rpms=()
        shopt -s nullglob
        llvmir_rpms=("$package_dir"/*-llvmir-*.rpm)
        shopt -u nullglob
        if ((${#llvmir_rpms[@]})); then
            printf 'PASS  %-12s %d LLVM IR RPM(s)\n' "$package" "${#llvmir_rpms[@]}"
            printf '%s\tPASS\t%s\t%d\t%s\n' "$package" "$srpm" "${#llvmir_rpms[@]}" "$log" >> "$summary"
        else
            printf 'FAIL  %-12s build succeeded but generated no *-llvmir-*.rpm; log: %s\n' "$package" "$log" >&2
            printf '%s\tFAIL\t%s\t0\t%s\n' "$package" "$srpm" "$log" >> "$summary"
            failures=$((failures + 1))
        fi
    else
        status=$?
        printf 'FAIL  %-12s Mock exited %d; log: %s\n' "$package" "$status" "$log" >&2
        printf '%s\tFAIL\t%s\t0\t%s\n' "$package" "$srpm" "$log" >> "$summary"
        failures=$((failures + 1))
    fi
done

printf '\nSummary: %s\n' "$summary"
if ((failures)); then
    printf '%d package(s) failed regression checks.\n' "$failures" >&2
    exit 1
fi
printf 'All %d package(s) passed.\n' "${#selected_packages[@]}"