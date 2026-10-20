#!/usr/bin/env bash
# Raw requests to OpenRouter's /systemone, outside Rig and only for this spike:
# does the endpoint read the `provider` routing object Rig 0.45 can't send?
# Reuses the body main.rs wrote. Saves status and body, never headers.
set -euo pipefail
cd "$(dirname "$0")"
: "${OPENROUTER_API_KEY:?OPENROUTER_API_KEY is not set}"

send() { # name, jq filter applied to the request body
  local name=$1 filter=$2
  local status
  status=$(jq "$filter" replies/request.json |
    curl -sS -o "replies/raw-$name.json" -w '%{http_code}' \
      -H "Authorization: Bearer $OPENROUTER_API_KEY" \
      -H 'Content-Type: application/json' \
      --data-binary @- https://openrouter.ai/api/v1/systemone)
  echo "$name: HTTP $status"
}

send baseline '.'
send zdr '.provider = {zdr: true, data_collection: "deny"}'
send impossible-route '.provider = {only: ["openai"], allow_fallbacks: false}'
send zdr-and-impossible-route '.provider = {zdr: true, data_collection: "deny", only: ["openai"], allow_fallbacks: false}'
