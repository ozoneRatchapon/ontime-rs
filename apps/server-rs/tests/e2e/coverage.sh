#!/usr/bin/env bash
# Probes every endpoint apps/client can call and reports its status.
set -u
B=${1:-http://127.0.0.1:8787}
RID=$(curl -s "$B/data/rundowns" | jq -r .loaded)

probe() { # method path
  local code
  code=$(curl -s -o /dev/null -w '%{http_code}' -m 10 -X "$1" "$B$2" -H 'content-type: application/json' -d '{}')
  printf '%-6s %-45s %s\n' "$1" "$2" "$code"
}

echo "--- read paths (client fetches these on load) ---"
for p in /data/rundowns /data/rundowns/current "/data/rundowns/$RID" /data/project /data/settings \
         /data/view-settings /data/custom-fields /data/url-presets /data/custom-views \
         /data/session/info /data/report /data/assets/css /data/automations /data/db/all \
         /data/settings/serverport /user/translations/translations.json; do
  probe GET "$p"
done

echo
echo "--- entry mutations ---"
probe POST  "/data/rundowns/$RID/entry"
probe PUT   "/data/rundowns/$RID/entry"
probe PUT   "/data/rundowns/$RID/batch"
probe PATCH "/data/rundowns/$RID/reorder"
probe PATCH "/data/rundowns/$RID/swap"
probe PATCH "/data/rundowns/$RID/renumber"
probe POST  "/data/rundowns/$RID/group"

echo
echo "--- not ported (expected to fail) ---"
for spec in "POST /data/rundowns/import" "POST /data/excel/upload" "GET /data/excel/$RID/export" \
            "GET /data/sheets/connect" "POST /data/db/upload" "POST /data/db/new" \
            "POST /data/automations/trigger" "POST /data/assets/css"; do
  probe ${spec%% *} "${spec#* }"
done
