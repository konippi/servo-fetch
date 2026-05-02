#!/usr/bin/env bash
set -euo pipefail

if (( $# > 1 )); then
  echo "Error: too many arguments: expected one of --dry-run|--apply|--help" >&2
  exit 2
fi

DRY_RUN=
case "${1:-}" in
  --dry-run) DRY_RUN=1 ;;
  --apply) DRY_RUN=0 ;;
  ""|--help|-h)
    cat >&2 <<'USAGE'
Usage: sync-labels.sh <--dry-run|--apply>
  --dry-run  preview changes without modifying labels
  --apply    apply changes to the repository (writes to GitHub)

Required env: GH_TOKEN, GH_REPO
Optional env: LABELS_FILE (default: .github/labels.yml)
USAGE
    exit 2 ;;
  *)
    echo "Error: unknown argument: $1 (run --help for usage)" >&2
    exit 2 ;;
esac

: "${GH_TOKEN:?GH_TOKEN must be set}"
: "${GH_REPO:?GH_REPO must be set (owner/repo)}"

readonly LABELS_FILE="${LABELS_FILE:-.github/labels.yml}"
readonly MAX_LABELS=100 MAX_NAME_LEN=50 MAX_DESC_LEN=100

command -v gh >/dev/null || { echo "Error: gh CLI not found" >&2; exit 1; }
command -v yq >/dev/null || { echo "Error: yq not found" >&2; exit 1; }
[[ -r "$LABELS_FILE" ]] || { echo "Error: cannot read ${LABELS_FILE}" >&2; exit 1; }

kind=$(yq 'type' "$LABELS_FILE")
if [[ "$kind" != "!!seq" ]]; then
  echo "Error: ${LABELS_FILE} must be a YAML sequence (got ${kind})" >&2
  exit 1
fi

total=$(yq 'length' "$LABELS_FILE")
if ! [[ "$total" =~ ^[0-9]+$ ]] || (( total == 0 )); then
  echo "Error: ${LABELS_FILE} is empty or malformed" >&2
  exit 1
fi
if (( total > MAX_LABELS )); then
  echo "Error: ${LABELS_FILE} declares ${total} labels (limit ${MAX_LABELS})" >&2
  exit 1
fi

declare -A existing=()
while IFS= read -r n; do
  [[ -n "$n" ]] && existing["$n"]=1
done < <(gh label list --limit 200 --json name --jq '.[].name')

declare -A seen=()
summary_file="${GITHUB_STEP_SUMMARY:-/dev/null}"
{
  echo "# Label sync report"
  echo ""
  echo "| Action | Label | Color |"
  echo "| --- | --- | --- |"
} >> "$summary_file"

created=0 updated=0 skipped=0 index=-1

while IFS=$'\t' read -r name color desc; do
  index=$((index + 1))

  if [[ -z "$name" || "$name" == "null" || "$name" == "~" ]]; then
    echo "Error: label[${index}] has empty or null name" >&2
    exit 1
  fi
  if (( ${#name} > MAX_NAME_LEN )); then
    echo "Error: label name too long: '${name}' (${#name} chars, max ${MAX_NAME_LEN})" >&2
    exit 1
  fi
  if [[ "$name" =~ [[:cntrl:]] ]]; then
    echo "Error: label[${index}] name contains control characters" >&2
    exit 1
  fi
  if (( ${#desc} > MAX_DESC_LEN )); then
    echo "Error: label '${name}' description too long (${#desc} chars, max ${MAX_DESC_LEN})" >&2
    exit 1
  fi
  if ! [[ "$color" =~ ^[0-9a-fA-F]{6}$ ]]; then
    echo "Error: label '${name}' has invalid color '${color}'" >&2
    exit 1
  fi
  if [[ -n "${seen[$name]:-}" ]]; then
    echo "Error: duplicate label name in ${LABELS_FILE}: '${name}'" >&2
    exit 1
  fi
  seen["$name"]=1

  if [[ -n "${existing[$name]:-}" ]]; then
    action="edit"
  else
    action="create"
  fi

  echo "[${action}] ${name} (#${color})"
  echo "| ${action} | \`${name}\` | \`#${color}\` |" >> "$summary_file"

  if (( DRY_RUN == 1 )); then
    skipped=$((skipped + 1))
    continue
  fi

  case "$action" in
    create)
      gh label create "$name" --color "$color" --description "$desc"
      created=$((created + 1))
      ;;
    edit)
      gh label edit "$name" --color "$color" --description "$desc"
      updated=$((updated + 1))
      ;;
  esac
done < <(yq -r '.[] | [.name, .color, (.description // "")] | @tsv' "$LABELS_FILE")

{
  echo ""
  echo "**Totals:** created=${created}, updated=${updated}, skipped(dry-run)=${skipped}, declared=${total}"
} >> "$summary_file"

echo "Done: created=${created}, updated=${updated}, skipped=${skipped}, declared=${total}"
