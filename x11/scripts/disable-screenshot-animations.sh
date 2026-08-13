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

ensure_rule_in_general() {
  local group="$1"
  local rules
  rules="$(kreadconfig5 --file kwinrulesrc --group General --key rules --default "")"

  local -a input=()
  if [[ -n "${rules}" ]]; then
    IFS=',' read -r -a input <<<"${rules}"
  fi

  local -a out=()
  local id
  local found=0
  for id in "${input[@]}"; do
    id="${id// /}"
    [[ -z "${id}" ]] && continue
    if [[ "${id}" == "${group}" ]]; then
      found=1
    fi
    out+=("${id}")
  done

  if [[ "${found}" -eq 0 ]]; then
    out+=("${group}")
  fi

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
  count="$(kreadconfig5 --file kwinrulesrc --group General --key count --default 0)"
  if [[ "${count}" =~ ^[0-9]+$ ]]; then
    rule_group="$((count + 1))"
  else
    rule_group="1"
  fi
fi

kwriteconfig5 --file kwinrulesrc --group "${rule_group}" --key Description "${RULE_MARKER}"
kwriteconfig5 --file kwinrulesrc --group "${rule_group}" --key title "${WINDOW_TITLE}"
kwriteconfig5 --file kwinrulesrc --group "${rule_group}" --key titlematch 1
kwriteconfig5 --file kwinrulesrc --group "${rule_group}" --key blockcompositing true
kwriteconfig5 --file kwinrulesrc --group "${rule_group}" --key blockcompositingrule 2

ensure_rule_in_general "${rule_group}"
reconfigure_kwin

echo "Disabled animations for '${WINDOW_TITLE}' via KWin rule group [${rule_group}]."
