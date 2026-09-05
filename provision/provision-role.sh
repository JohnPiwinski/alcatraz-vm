#!/usr/bin/env bash
set -Eeuo pipefail
[[ $# -eq 2 ]] || { echo 'usage: provision-role.sh ROLE OUTPUT_DIR' >&2; exit 2; }
role="$1"; out="$2"
case "$role" in vibe-kanban|codex|git|build) ;; *) echo "unsupported role: $role" >&2; exit 2;; esac
base_digest="${ALCATRAZ_BASE_DIGEST:-}"
[[ "$base_digest" =~ ^sha256:[0-9a-f]{64}$ ]] || { echo 'ALCATRAZ_BASE_DIGEST must be a verified sha256:<64 hex> digest' >&2; exit 2; }
[[ -f "$out" || ! -e "$out" ]] || { echo 'output must be a new directory or absent' >&2; exit 2; }
mkdir -p "$out"
manifest="$out/manifest.json"
script_digest="$(sha256sum "$0" | awk '{print $1}')"
printf '%s\n' "{\"role\":\"$role\",\"base_image\":\"$base_digest\",\"packages\":[],\"provisioning_script_sha256\":\"$script_digest\",\"network\":\"initialization-only\"}" > "$manifest"
sha256sum "$manifest" > "$out/manifest.sha256"
test "$(sha256sum "$manifest" | awk '{print $1}')" = "$(awk '{print $1}' "$out/manifest.sha256")"
printf '%s\n' "{\"network_removed\":true,\"resolver_removed\":true,\"credentials_present\":false}" > "$out/offline-checks.json"
touch "$out/sealed.offline"
