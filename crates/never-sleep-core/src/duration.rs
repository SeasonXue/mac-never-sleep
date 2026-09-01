use chrono::{Local, NaiveTime};

use crate::i18n::Lang;
use crate::DurationPref;

/// 按本地日历计算下一个墙上时钟 HH:MM（处理夏令时切日）。
pub fn next_until_wallclock(hour: u8, minute: u8) -> i64 {
    let now = Local::now();
    let time = NaiveTime::from_hms_opt(u32::from(hour), u32::from(minute), 0)
        .unwrap_or(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    let today = now.date_naive();
    let resolve = |date: chrono::NaiveDate| {
        date.and_time(time)
            .and_local_timezone(Local)
            .earliest()
            .or_else(|| date.and_time(time).and_local_timezone(Local).latest())
    };
    if let Some(dt) = resolve(today) {
        if dt > now {
            return dt.timestamp();
        }
    }
    let tomorrow = today.succ_opt().unwrap_or(today);
    resolve(tomorrow)
        .map(|d| d.timestamp())
        .unwrap_or_else(|| now.timestamp() + 24 * 3600)
}

/// 固定时区偏移下的下一个 HH:MM（单测用，不处理夏令时）。
/// `offset_secs` 为本地相对 UTC 的偏移（东八区 = 28800）。
pub fn next_until_unix_secs(now_unix: i64, offset_secs: i32, hour: u8, minute: u8) -> i64 {
    let local = now_unix + i64::from(offset_secs);
    let tod = local.rem_euclid(86_400);
    let target = i64::from(hour) * 3600 + i64::from(minute) * 60;
    let mut delta = target - tod;
    if delta <= 0 {
        delta += 86_400;
    }
    now_unix + delta
}

pub fn deadline_unix_secs(
    _now_unix: i64,
    _offset_secs: i32,
    started_unix: i64,
    pref: DurationPref,
) -> Option<i64> {
    match pref {
        DurationPref::Indefinite => None,
        DurationPref::Hours { hours } => Some(started_unix + i64::from(hours) * 3600),
        DurationPref::UntilLocal { hour, minute } => Some(next_until_wallclock(hour, minute)),
    }
}

pub fn format_duration(lang: Lang, secs: u64) -> String {
    match lang {
        Lang::En => format_duration_en(secs),
        Lang::Zh => format_duration_zh(secs),
    }
}

/// Compact elapsed clock for the moon-panel timer. Language-independent digits.
pub fn format_clock(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let rem = secs % 60;
    if hours == 0 {
        format!("{mins}:{rem:02}")
    } else {
        format!("{hours}:{mins:02}:{rem:02}")
    }
}

/// Countdown clock that keeps the hour column so 1:00:00 does not jump to 59:59.
pub fn format_countdown(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let rem = secs % 60;
    format!("{hours}:{mins:02}:{rem:02}")
}

/// Milliseconds left in a timed session, measured on the monotonic clock.
///
/// `deadline_unix - started_unix` is the planned length. Subtracting elapsed
/// monotonic time keeps the UI from skipping seconds when `unix_secs` truncates
/// or the wall clock steps.
pub fn remaining_ms(deadline_unix: i64, started_unix: i64, started_ms: u64, now_ms: u64) -> u64 {
    let duration_ms = deadline_unix.saturating_sub(started_unix).max(0) as u64 * 1_000;
    duration_ms.saturating_sub(now_ms.saturating_sub(started_ms))
}

/// Whole seconds shown on a countdown. Holds the current second until it fully elapses.
pub fn countdown_secs(remaining_ms: u64) -> u64 {
    remaining_ms.div_ceil(1_000)
}

pub fn elapsed_secs(started_ms: u64, now_ms: u64) -> u64 {
    now_ms.saturating_sub(started_ms) / 1_000
}

/// Remaining milliseconds for the active duration kind.
///
/// `Hours` follows the monotonic clock so the UI does not skip seconds when
/// `unix_secs` truncates. `UntilLocal` follows the wall clock, matching the stop
/// condition for “until 08:00”.
pub fn session_remaining_ms(
    pref: DurationPref,
    deadline_unix: i64,
    started_unix: i64,
    started_ms: u64,
    now_ms: u64,
    now_unix: i64,
) -> u64 {
    match pref {
        DurationPref::Hours { .. } => remaining_ms(deadline_unix, started_unix, started_ms, now_ms),
        DurationPref::UntilLocal { .. } => {
            deadline_unix.saturating_sub(now_unix).max(0) as u64 * 1_000
        }
        DurationPref::Indefinite => 0,
    }
}

fn format_duration_en(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs} sec");
    }
    let mins = secs / 60;
    if mins < 60 {
        let rem = secs % 60;
        if rem == 0 {
            format!("{mins} min")
        } else {
            format!("{mins} min {rem} sec")
        }
    } else {
        let hours = mins / 60;
        let rem_m = mins % 60;
        if rem_m == 0 {
            format!("{hours} hr")
        } else {
            format!("{hours} hr {rem_m} min")
        }
    }
}

fn format_duration_zh(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs} 秒");
    }
    let mins = secs / 60;
    if mins < 60 {
        let rem = secs % 60;
        if rem == 0 {
            format!("{mins} 分钟")
        } else {
            format!("{mins} 分 {rem} 秒")
        }
    } else {
        let hours = mins / 60;
        let rem_m = mins % 60;
        if rem_m == 0 {
            format!("{hours} 小时")
        } else {
            format!("{hours} 小时 {rem_m} 分")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn until_rolls_to_tomorrow() {
        let offset = 8 * 3600;
        // unix such that local = 18:00
        let local_1800 = 18 * 3600;
        let now_unix = local_1800 - i64::from(offset); // 10:00 UTC
        let next = next_until_unix_secs(now_unix, offset, 8, 0);
        // delta = 08:00 next day from 18:00 = 14 hours
        assert_eq!(next - now_unix, 14 * 3600);
    }

    #[test]
    fn until_today_if_still_ahead() {
        let offset = 8 * 3600;
        let local_0700 = 7 * 3600;
        let now_unix = local_0700 - i64::from(offset);
        let next = next_until_unix_secs(now_unix, offset, 8, 0);
        assert_eq!(next - now_unix, 3600);
    }

    #[test]
    fn format_examples() {
        assert_eq!(format_duration(Lang::Zh, 8), "8 秒");
        assert_eq!(format_duration(Lang::Zh, 60), "1 分钟");
        assert_eq!(format_duration(Lang::Zh, 90), "1 分 30 秒");
        assert_eq!(format_duration(Lang::Zh, 3600), "1 小时");
        assert_eq!(format_duration(Lang::Zh, 3720), "1 小时 2 分");
        assert_eq!(format_duration(Lang::En, 8), "8 sec");
        assert_eq!(format_duration(Lang::En, 60), "1 min");
        assert_eq!(format_duration(Lang::En, 90), "1 min 30 sec");
        assert_eq!(format_duration(Lang::En, 3600), "1 hr");
        assert_eq!(format_duration(Lang::En, 3720), "1 hr 2 min");
    }

    #[test]
    fn format_clock_is_compact_elapsed() {
        assert_eq!(format_clock(0), "0:00");
        assert_eq!(format_clock(5), "0:05");
        assert_eq!(format_clock(65), "1:05");
        assert_eq!(format_clock(3599), "59:59");
        assert_eq!(format_clock(3600), "1:00:00");
        assert_eq!(format_clock(3725), "1:02:05");
    }

    #[test]
    fn wallclock_is_in_the_future() {
        let now = chrono::Local::now().timestamp();
        let next = next_until_wallclock(8, 0);
        assert!(next > now);
        assert!(next - now <= 24 * 3600 + 60);
    }

    #[test]
    fn deadline_unix_secs_for_each_pref() {
        assert_eq!(
            deadline_unix_secs(0, 0, 1_000, DurationPref::Indefinite),
            None
        );
        assert_eq!(
            deadline_unix_secs(0, 0, 1_000, DurationPref::Hours { hours: 2 }),
            Some(1_000 + 7_200)
        );
        let until =
            deadline_unix_secs(0, 0, 1_000, DurationPref::UntilLocal { hour: 8, minute: 0 });
        assert!(until.is_some());
        assert!(until.unwrap() > chrono::Local::now().timestamp() - 1);
    }

    #[test]
    fn next_until_unix_secs_at_exact_target_rolls_forward() {
        let offset = 0;
        let now_unix = 8 * 3600;
        let next = next_until_unix_secs(now_unix, offset, 8, 0);
        assert_eq!(next - now_unix, 86_400);
    }

    #[test]
    fn remaining_ms_follows_monotonic_not_wall_clock() {
        let started_unix = 1_700_000_000;
        let started_ms = 5_000;
        let deadline = started_unix + 3_600;
        // Wall clock jumped 5s; monotonic only advanced 2s.
        let remaining = remaining_ms(deadline, started_unix, started_ms, started_ms + 2_000);
        assert_eq!(remaining, 3_598_000);
        assert_eq!(countdown_secs(remaining), 3_598);
        assert_eq!(
            elapsed_secs(started_ms, started_ms + 2_000) + countdown_secs(remaining),
            3_600,
            "elapsed + remaining must stay on the chosen duration"
        );
    }

    #[test]
    fn countdown_secs_holds_the_second_until_it_elapses() {
        assert_eq!(countdown_secs(0), 0);
        assert_eq!(countdown_secs(1), 1);
        assert_eq!(countdown_secs(1_000), 1);
        assert_eq!(countdown_secs(1_001), 2);
        assert_eq!(countdown_secs(3_600_000), 3_600);
        assert_eq!(
            countdown_secs(3_599_001),
            3_600,
            "the opening 1:00:00 must last a full second"
        );
        assert_eq!(countdown_secs(3_599_000), 3_599);
    }

    #[test]
    fn format_countdown_keeps_the_hour_column() {
        assert_eq!(format_countdown(3_600), "1:00:00");
        assert_eq!(format_countdown(3_599), "0:59:59");
        assert_eq!(format_countdown(5), "0:00:05");
    }

    #[test]
    fn until_local_remaining_follows_wall_clock() {
        let started_unix = 1_700_000_000;
        let deadline = started_unix + 3_600;
        let rem = session_remaining_ms(
            DurationPref::UntilLocal { hour: 8, minute: 0 },
            deadline,
            started_unix,
            0,
            5_000,
            started_unix + 10,
        );
        assert_eq!(
            rem, 3_590_000,
            "UntilLocal remaining must drop with unix_secs, not monotonic ms"
        );
        let hours = session_remaining_ms(
            DurationPref::Hours { hours: 1 },
            deadline,
            started_unix,
            0,
            5_000,
            started_unix + 10,
        );
        assert_eq!(hours, 3_595_000);
    }
}
