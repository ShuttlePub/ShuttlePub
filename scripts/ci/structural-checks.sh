#!/usr/bin/env bash
# Layering checks read from Cargo's dependency graph:
# - each workspace crate depends only on the workspace crates its layer allows
#   (server -> {application, driver} -> kernel);
# - kernel does not depend, directly or transitively, on an async runtime, a
#   web framework or an HTTP client.
# Every violation is printed, and the script exits non-zero when there is any.
set -Eeuo pipefail

cd "$(git rev-parse --show-toplevel)"

workspace_crates=(
  shuttlepub-kernel
  shuttlepub-application
  shuttlepub-driver
  shuttlepub-server
)

declare -A allowed_workspace_deps=(
  [shuttlepub-kernel]=""
  [shuttlepub-application]="shuttlepub-kernel"
  [shuttlepub-driver]="shuttlepub-kernel"
  [shuttlepub-server]="shuttlepub-application shuttlepub-driver shuttlepub-kernel"
)

kernel_forbidden_deps=(tokio axum reqwest)

is_workspace_crate() {
  local name="$1" crate
  for crate in "${workspace_crates[@]}"; do
    [[ "$name" == "$crate" ]] && return 0
  done
  return 1
}

# Prints the package names `cargo tree` lists for a crate, one per line,
# without the crate itself.
dependency_names() {
  local crate="$1"
  shift
  cargo tree --locked -p "$crate" --prefix none "$@" \
    | tail -n +2 \
    | awk '{ print $1 }' \
    | sort -u
}

main() {
  local violations=0 crate dep tree

  for crate in "${workspace_crates[@]}"; do
    tree="$(dependency_names "$crate" -e normal,build,dev --depth 1)"
    while IFS= read -r dep; do
      [[ -n "$dep" ]] || continue
      is_workspace_crate "$dep" || continue
      if [[ " ${allowed_workspace_deps[$crate]} " != *" $dep "* ]]; then
        printf 'layer: %s must not depend on %s\n' "$crate" "$dep" >&2
        violations=$((violations + 1))
      fi
    done <<<"$tree"
  done

  tree="$(dependency_names shuttlepub-kernel -e normal,build)"
  for dep in "${kernel_forbidden_deps[@]}"; do
    if grep -qx -- "$dep" <<<"$tree"; then
      printf 'layer: shuttlepub-kernel must not depend on %s\n' "$dep" >&2
      violations=$((violations + 1))
    fi
  done

  if ((violations > 0)); then
    printf 'structural checks: %d violation(s)\n' "$violations" >&2
    exit 1
  fi
}

main "$@"
