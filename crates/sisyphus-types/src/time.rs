//! UTC 时间戳工具。
//!
//! 统一实现 ISO-8601 时间戳，避免各 crate 各写一份。历史上
//! `sisyphus-tools-session`（三处）与 `sisyphus-tools-llm-tools`（一处）各自
//! 拷贝了同一段手写日历算法，其中月份/日期用 `days % 365 / 30.44` 近似，
//! 跨月与闰年会产出不存在的日期（例如一年中的第 200 天被算成 7 月 17 日）。
//! 授权过期判断依赖 ISO 字符串的字典序；正确的日期同样保持单调，因此
//! 修正算法不会破坏既有比较逻辑，只是让持久化的时间戳可读、可解析。
//!
//! 实现基于 `jiff`（而非 chrono）：chrono 全版本已进 RUSTSEC-2026-0543
//! （unmaintained）。jiff 的 `Timestamp` 是正确的 UTC 时刻表示，输出格式为
//! RFC 3339 —— 与旧实现保持「`YYYY-MM-DDTHH:MM:SS.mmmZ`、固定毫秒精度」，
//! 不用 `Timestamp` 的 Display（它会裁掉尾零，导致长度不固定）。

use jiff::{tz::TimeZone, Timestamp};

/// 当前 UTC 时间，ISO-8601 / RFC 3339 格式，毫秒精度，以 `Z` 结尾。
///
/// 固定 3 位小数（`2026-11-18T12:34:56.789Z`，共 24 字符），与迁移前的
/// `chrono` 实现逐字节一致；测试依赖这个不变量（部分持久化数据按固定宽度
/// 解析，例如 `recorded_at` 的月份前缀切片）。
pub fn now_iso() -> String {
    let ts = Timestamp::now();
    let dt = TimeZone::UTC.to_datetime(ts);
    let date = dt.date();
    let time = dt.time();
    // 手工拼接而非 `civil::DateTime` 的 Display：后者会带纳秒小数
    // （`.002791818`），会让长度随纳秒漂移。这里固定毫秒 3 位。
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        date.year(),
        date.month(),
        date.day(),
        time.hour(),
        time.minute(),
        time.second(),
        ts.subsec_nanosecond() / 1_000_000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_iso_is_valid_rfc3339_utc_with_millis() {
        let ts = now_iso();
        assert_eq!(ts.len(), 24, "expected YYYY-MM-DDTHH:MM:SS.mmmZ, got {ts}");
        assert!(ts.ends_with('Z'), "UTC timestamps must end with Z: {ts}");
        let parsed: Timestamp = ts.parse().expect("valid RFC 3339");
        assert_eq!(parsed.subsec_nanosecond() % 1_000_000, 0);
    }

    #[test]
    fn test_now_iso_dates_are_within_one_day_of_each_other() {
        let a: Timestamp = now_iso().parse().expect("valid RFC 3339");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b: Timestamp = now_iso().parse().expect("valid RFC 3339");
        let delta = b.as_millisecond() - a.as_millisecond();
        assert!(
            (0..5_000).contains(&delta),
            "unexpected clock delta: {delta}ms"
        );
    }

    #[test]
    fn test_now_iso_roundtrips_to_the_same_instant() {
        let ts = now_iso();
        let parsed: Timestamp = ts.parse().expect("parseable");
        let dt = TimeZone::UTC.to_datetime(parsed);
        let date = dt.date();
        let time = dt.time();
        let expected = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            date.year(),
            date.month(),
            date.day(),
            time.hour(),
            time.minute(),
            time.second(),
            parsed.subsec_nanosecond() / 1_000_000
        );
        assert_eq!(expected, ts);
    }
}
