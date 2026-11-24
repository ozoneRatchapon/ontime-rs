# Draft: upstream issue — runtime offset is a day off for events started after midnight

Not posted. This is for the owner to post at github.com/cpvalente/ontime/issues
if they choose. It was found by RIIR 4, from the case "after midnight" in
`packages/ontime-core/tests/fixtures/runtime_offset.json`, which was generated
from upstream master `0ca4d79b2`.

---

**Title:** Runtime offset reads ~24h early for an event started after midnight

**Steps**

1. Make a rundown that crosses midnight:
   - A at 23:30 for 60 min;
   - B at 00:45 for 30 min, so B gets `dayOffset: 1`.
2. Start A at 23:40, so the show is 10 minutes late.
3. After midnight, start B at 00:50, 5 minutes late.

**Expected:** `offset.absolute` ≈ +5 min.

**Actual:** `offset.absolute` = −86 100 000 ms (−23:55), and `offset.relative`
= −86 700 000 ms.

**Cause:** `getRuntimeOffset` in
`apps/server/src/services/runtime-service/timer.utils.ts`, line 232:

```ts
const eventStartOffset = startedAt + startDayOffset * dayInMs - (timeStart + dayOffset * dayInMs);
```

- `startDayOffset` is the day the *show* began.
- `startedAt` is a time of day on the day the *event* started, which here is
  one day later. The event's planned side uses its own `dayOffset`, so the two
  sides are a day apart.

**Suggested fix:** use the day the event started:

```ts
// the show's first day plus the midnights between the show start and this event's start
const eventStartDay = startDayOffset + daysBetween(startEpoch, eventStartEpoch);
const eventStartOffset = startedAt + eventStartDay * dayInMs - (timeStart + dayOffset * dayInMs);
```

`relative` keeps `startDayOffset`, because it compares against the show's start.

---

**Our fork:** fixed in `packages/ontime-core/src/offset.rs`, through
`OffsetInputs::started_day`. It is recorded as a deliberate difference in
`tests/runtime_offset_tests.rs`.
