#!/usr/bin/env bash
# Vendors the Logseq documentation pages that define the Markdown dialect and
# its rendering, so the repo carries a reference that can be refreshed on demand.
#
# The upstream repo (github.com/logseq/docs) is MIT licensed; a copy of its
# licence sits next to the snapshot. Only documentation is fetched — never
# Logseq's source code, which is AGPL and off-limits for this repo.
#
# Usage:
#   scripts/sync-logseq-docs.sh            # refresh the snapshot from master
#   scripts/sync-logseq-docs.sh --check    # exit 1 when upstream moved
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/docs/reference/logseq"
upstream="https://github.com/logseq/docs"
branch="master"

# Pages that define the Markdown dialect, its properties and its rendering.
# Keep this list curated: the whole upstream graph is 240+ pages of product
# documentation, most of it irrelevant to the file format.
pages=(
  "Markdown"
  "Properties"
  "Property"
  "Built-in Properties"
  "Filename format"
  "Logseq file and folder naming rules"
  "Namespaces"
  "How to create a Logseq graph using existing Markdown files"
  "Block Reference"
  "Page and block references"
  "The basics of block references"
  "The difference between block embeds and block references"
  "The difference between block embeds and page embeds"
  "Tags"
  "Aliases and external links"
  "Linked References"
  "Unlinked References"
  "Tasks"
  "Code block"
  "Math Block"
  "Tables"
  "Headings"
  "Numbered List"
  "Hiccup"
  "Macros"
  "Embed Media - Audio, Photos, Videos"
  "Cloze"
  "Flashcards"
  "Spaced Repetition"
  "Templates___Docs"
  "Queries"
  "Query table"
  "config edn file"
  "Global config.edn"
  "setting___preferred file format"
  "setting___preferred journal format"
  "Export"
  "Org Mode|org"
  "term___page reference"
  "term___page reference with label"
  "term___block reference with label"
  "term___internal line break"
  "term___block rendering"
  "term___block title"
  "term___block"
  "term___bullet"
  "term___file link"
  "term___web link"
  "term___alias"
  "term___page title"
  "term___editing mode"
)

urlencode() {
  local s="$1"
  s="${s//%/%25}"
  s="${s// /%20}"
  printf '%s' "$s"
}

upstream_sha() {
  git ls-remote "$upstream" "refs/heads/$branch" | cut -f1
}

manifest="$out/MANIFEST.md"
current_sha=""
if [[ -f "$manifest" ]]; then
  current_sha="$(sed -n 's/^- commit: `\([0-9a-f]\{40\}\)`$/\1/p' "$manifest" | head -1)"
fi

sha="$(upstream_sha)"
if [[ -z "$sha" ]]; then
  echo "could not resolve $upstream $branch" >&2
  exit 1
fi

if [[ "${1:-}" == "--check" ]]; then
  if [[ "$sha" == "$current_sha" ]]; then
    echo "logseq docs up to date ($sha)"
    exit 0
  fi
  echo "logseq docs moved: manifest $current_sha -> upstream $sha" >&2
  echo "run scripts/sync-logseq-docs.sh to refresh" >&2
  exit 1
fi

mkdir -p "$out/pages"
changed=0
for entry in "${pages[@]}"; do
  page="${entry%%|*}"
  ext="md"
  [[ "$entry" == *"|"* ]] && ext="${entry##*|}"
  encoded="$(urlencode "$page")"
  url="https://raw.githubusercontent.com/logseq/docs/$sha/pages/$encoded.$ext"
  tmp="$(mktemp)"
  if ! curl -fsS -m 30 "$url" -o "$tmp"; then
    echo "missing upstream page: $page.$ext" >&2
    rm -f "$tmp"
    exit 1
  fi
  dest="$out/pages/$page.$ext"
  if [[ -f "$dest" ]] && cmp -s "$tmp" "$dest"; then
    rm -f "$tmp"
    continue
  fi
  mv "$tmp" "$dest"
  changed=$((changed + 1))
done

curl -fsS -m 30 "https://raw.githubusercontent.com/logseq/docs/$sha/LICENSE.md" \
  -o "$out/LICENSE.md"

{
  echo "# Logseq 文档快照"
  echo
  echo "由 \`scripts/sync-logseq-docs.sh\` 生成，不要手改。"
  echo
  echo "- source: $upstream"
  echo "- branch: \`$branch\`"
  echo "- commit: \`$sha\`"
  echo "- fetched: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- pages: ${#pages[@]}"
  echo "- licence: MIT (见 \`LICENSE.md\`)"
  echo
  echo "## 文件"
  echo
  echo "| 文件 | sha256 |"
  echo "| --- | --- |"
  for entry in "${pages[@]}"; do
    page="${entry%%|*}"
    ext="md"
    [[ "$entry" == *"|"* ]] && ext="${entry##*|}"
    sum="$(sha256sum "$out/pages/$page.$ext" | cut -d' ' -f1)"
    echo "| \`pages/$page.$ext\` | \`$sum\` |"
  done
} > "$manifest"

echo "synced ${#pages[@]} pages from $sha ($changed changed)"
