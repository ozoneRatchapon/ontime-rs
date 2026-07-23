#!/usr/bin/env bash
# End-to-end check of the rundown REST surface against a running worker.
# Works on a dedicated rundown so it is safe to re-run against persisted state.
set -u
B=${1:-http://127.0.0.1:8787}
fail=0

req() { # method path [body]
  local m=$1 p=$2 body=${3:-}
  if [ -n "$body" ]; then
    curl -s -m 15 -X "$m" "$B$p" -H 'content-type: application/json' -d "$body"
  else
    curl -s -m 15 -X "$m" "$B$p"
  fi
}

check() { # label actual expected
  if [ "$2" = "$3" ]; then
    echo "  ok   $1"
  else
    echo "  FAIL $1 -> got '$2' want '$3'"
    fail=1
  fi
}

# index of an id inside a json array, or -1
idx() { jq -r --arg id "$2" '(index($id) // -1)' <<<"$1"; }

echo "== a fresh rundown for this run =="
LIST=$(req POST /data/rundowns '{"title":"__smoke__"}')
RID=$(jq -r '.rundowns[]|select(.title=="__smoke__").id' <<<"$LIST" | tail -1)
check "rundown created" "$(test -n "$RID" && echo yes)" "yes"
req POST "/data/rundowns/$RID/load" >/dev/null
check "rundown loaded"  "$(req GET /data/rundowns | jq -r .loaded)" "$RID"

echo "== current rundown is valid for the client =="
CUR=$(req GET /data/rundowns/current)
check "is the loaded one" "$(jq -r .id <<<"$CUR")" "$RID"
check "has order"      "$(jq -r 'has("order")' <<<"$CUR")" "true"
check "has flatOrder"  "$(jq -r 'has("flatOrder")' <<<"$CUR")" "true"
check "has entries"    "$(jq -r '.entries|type' <<<"$CUR")" "object"
check "has revision"   "$(jq -r '.revision|type' <<<"$CUR")" "number"
check "starts empty"   "$(jq -r '.order|length' <<<"$CUR")" "0"

echo "== create an entry =="
NEW=$(req POST "/data/rundowns/$RID/entry" '{"title":"SMOKE_A","duration":60000,"cue":"A"}')
AID=$(jq -r .id <<<"$NEW")
check "returns an id"   "$(test -n "$AID" && echo yes)" "yes"
check "valid timerType" "$(jq -r .timerType <<<"$NEW")" "count-down"
check "valid strategy"  "$(jq -r .timeStrategy <<<"$NEW")" "lock-duration"

echo "== the entry is actually persisted =="
check "present after refetch" "$(req GET "/data/rundowns/$RID" | jq -r ".entries[\"$AID\"].title")" "SMOKE_A"

echo "== edit the entry =="
req PUT "/data/rundowns/$RID/entry" "{\"id\":\"$AID\",\"title\":\"SMOKE_A_EDITED\"}" >/dev/null
EDITED=$(req GET "/data/rundowns/$RID" | jq -r ".entries[\"$AID\"]")
check "title updated"      "$(jq -r .title <<<"$EDITED")" "SMOKE_A_EDITED"
check "duration preserved" "$(jq -r .duration <<<"$EDITED")" "60000"

echo "== second entry + link timing =="
BID=$(req POST "/data/rundowns/$RID/entry" '{"title":"SMOKE_B","duration":30000,"linkStart":true}' | jq -r .id)
req PUT "/data/rundowns/$RID/entry" "{\"id\":\"$AID\",\"timeStart\":3600000}" >/dev/null
RD=$(req GET "/data/rundowns/$RID")
check "A ends at 1h+60s"     "$(jq -r ".entries[\"$AID\"].timeEnd" <<<"$RD")" "3660000"
check "B starts when A ends" "$(jq -r ".entries[\"$BID\"].timeStart" <<<"$RD")" "3660000"

echo "== reorder =="
req PATCH "/data/rundowns/$RID/reorder" "{\"entryId\":\"$BID\",\"destinationId\":\"$AID\",\"order\":\"before\"}" >/dev/null
ORDER=$(req GET "/data/rundowns/$RID" | jq -c .order)
check "B now precedes A" "$(test "$(idx "$ORDER" "$BID")" -lt "$(idx "$ORDER" "$AID")" && echo yes)" "yes"

echo "== group / ungroup =="
GROUPED=$(req POST "/data/rundowns/$RID/group" "{\"ids\":[\"$AID\",\"$BID\"]}")
GID=$(jq -r '.entries|to_entries[]|select(.value.type=="group").key' <<<"$GROUPED")
check "group created"      "$(jq -r ".entries[\"$GID\"].type" <<<"$GROUPED")" "group"
check "children reparented" "$(jq -r ".entries[\"$AID\"].parent" <<<"$GROUPED")" "$GID"
check "flatOrder expands"  "$(jq -r '.flatOrder|length' <<<"$GROUPED")" "$(jq -r '.entries|length' <<<"$GROUPED")"
check "group spans 90s"    "$(jq -r ".entries[\"$GID\"].duration" <<<"$GROUPED")" "90000"
UNGROUPED=$(req POST "/data/rundowns/$RID/ungroup/$GID")
check "group dissolved"    "$(jq -r ".entries|has(\"$GID\")" <<<"$UNGROUPED")" "false"
check "children survive"   "$(jq -r ".entries|has(\"$AID\")" <<<"$UNGROUPED")" "true"

echo "== clone =="
CLONED=$(req POST "/data/rundowns/$RID/clone/$AID")
check "one more entry" "$(jq -r '.entries|length' <<<"$CLONED")" "3"

echo "== delete =="
req DELETE "/data/rundowns/$RID/entries" "{\"ids\":[\"$AID\"]}" >/dev/null
check "entry gone" "$(req GET "/data/rundowns/$RID" | jq -r ".entries|has(\"$AID\")")" "false"
req DELETE "/data/rundowns/$RID/all" >/dev/null
check "rundown emptied" "$(req GET "/data/rundowns/$RID" | jq -r '.entries|length')" "0"

echo "== settings round trip =="
req POST /data/settings '{"timeFormat":"12"}' >/dev/null
check "timeFormat saved" "$(req GET /data/settings | jq -r .timeFormat)" "12"
check "language kept"    "$(req GET /data/settings | jq -r .language)" "en"
req POST /data/settings '{"timeFormat":"24"}' >/dev/null

echo "== project data round trip =="
req POST /data/project '{"title":"Smoke Project"}' >/dev/null
check "project title saved" "$(req GET /data/project | jq -r .title)" "Smoke Project"
check "logo field present"  "$(req GET /data/project | jq -r 'has("logo")')" "true"

echo "== view settings =="
check "has normalColor" "$(req GET /data/view-settings | jq -r 'has("normalColor")')" "true"

echo "== custom fields CRUD =="
req POST /data/custom-fields '{"type":"text","colour":"#fff","label":"Camera"}' >/dev/null
check "field created" "$(req GET /data/custom-fields | jq -r '.Camera.label')" "Camera"
req DELETE /data/custom-fields/Camera >/dev/null
check "field deleted" "$(req GET /data/custom-fields | jq -r 'has("Camera")')" "false"

echo "== url presets CRUD =="
req POST /data/url-presets '{"alias":"stage","target":"timer","enabled":true,"search":"","displayInNav":false}' >/dev/null
check "preset created" "$(req GET /data/url-presets | jq -r '[.[]|select(.alias=="stage")]|length')" "1"
req DELETE /data/url-presets/stage >/dev/null
check "preset deleted" "$(req GET /data/url-presets | jq -r '[.[]|select(.alias=="stage")]|length')" "0"

echo "== cleanup =="
req POST "/data/rundowns/default/load" >/dev/null
req DELETE "/data/rundowns/$RID" >/dev/null
check "smoke rundown removed" "$(req GET /data/rundowns | jq -r '[.rundowns[]|select(.id=="'"$RID"'")]|length')" "0"

echo
if [ $fail -eq 0 ]; then echo "ALL CHECKS PASSED"; else echo "SOME CHECKS FAILED"; fi
exit $fail
