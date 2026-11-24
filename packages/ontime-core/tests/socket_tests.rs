// Upstream 71beb53a: a Pong carries the server time of day, so a client can
// line its displayed clock up with the room's.
use ontime_core::socket::pong;
use serde_json::json;

const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;

#[test]
fn pong_echoes_the_ping_payload() {
    let reply = pong(json!(1234.5), 0, 0);
    assert_eq!(reply["tag"], "pong");
    assert_eq!(reply["payload"], json!(1234.5));
}

#[test]
fn pong_clock_is_time_of_day_in_the_room_timezone() {
    // 2026-11-24 10:15:00 UTC
    let now: i64 = 1_795_515_300_000;
    let utc = now.rem_euclid(DAY);
    assert_eq!(pong(json!(null), now, 0)["clock"], json!(utc));
    assert_eq!(
        pong(json!(null), now, 7 * HOUR)["clock"],
        json!(utc + 7 * HOUR)
    );
}

#[test]
fn pong_clock_wraps_past_midnight() {
    let just_before_midnight_utc = 20 * DAY + DAY - 1;
    assert_eq!(
        pong(json!(null), just_before_midnight_utc, HOUR)["clock"],
        json!(HOUR - 1)
    );
    assert_eq!(
        pong(json!(null), 20 * DAY, -HOUR)["clock"],
        json!(DAY - HOUR)
    );
}
