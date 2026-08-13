#!/usr/bin/env bash
set -euo pipefail

if ! command -v kwriteconfig5 >/dev/null 2>&1 || ! command -v kreadconfig5 >/dev/null 2>&1; then
  echo "Missing required tools: kwriteconfig5 and kreadconfig5" >&2
  exit 1
fi

CONFIG_FILE="${XDG_CONFIG_HOME:-${HOME}/.config}/kwinrulesrc"
RULE_MARKER="__SCREENSHOT_BLOCK_COMPOSITING__"
WINDOW_TITLE="screenshot"

find_rule_group() {
  if [[ ! -f "${CONFIG_FILE}" ]]; then
    return 1
  fi
  awk -F= -v marker="${RULE_MARKER}" '
    /^\[/ {
      section=$0
      gsub(/^\[/, "", section)
      gsub(/\]$/, "", section)
      next
    }
    $1 == "Description" && $2 == marker {
      print section
      exit
    }
  ' "${CONFIG_FILE}"
}

join_csv() {
  local IFS=","
  echo "$*"
}

remove_rule_from_general() {
  local group="$1"
  local rules
  rules="$(kreadconfig5 --file kwinrulesrc --group General --key rules --default "")"

  local -a input=()
  if [[ -n "${rules}" ]]; then
    IFS=',' read -r -a input <<<"${rules}"
  fi

  local -a out=()
  local id
  for id in "${input[@]}"; do
    id="${id// /}"
    [[ -z "${id}" ]] && continue
    [[ "${id}" == "${group}" ]] && continue
    out+=("${id}")
  done

  kwriteconfig5 --file kwinrulesrc --group General --key rules "$(join_csv "${out[@]}")"
  kwriteconfig5 --file kwinrulesrc --group General --key count "${#out[@]}"
}

reconfigure_kwin() {
  if command -v qdbus6 >/dev/null 2>&1; then
    qdbus6 org.kde.KWin /KWin reconfigure >/dev/null 2>&1 || true
  elif command -v qdbus >/dev/null 2>&1; then
    qdbus org.kde.KWin /KWin reconfigure >/dev/null 2>&1 || true
  fi
}

rule_group="$(find_rule_group || true)"
if [[ -z "${rule_group}" ]]; then
  echo "No screenshot animation rule found. Animations already enabled."
  exit 0
fi

# Leave the group for future reuse but disable/remove it from active rules.
kwriteconfig5 --file kwinrulesrc --group "${rule_group}" --key blockcompositing false
kwriteconfig5 --file kwinrulesrc --group "${rule_group}" --key blockcompositingrule 0
remove_rule_from_general "${rule_group}"
reconfigure_kwin

echo "Enabled animations for '${WINDOW_TITLE}' by deactivating KWin rule group [${rule_group}]."
