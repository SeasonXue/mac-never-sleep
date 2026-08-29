use crate::DurationPref;

/// 本地墙上时钟的下一个 HH:MM，以 unix 秒返回（UTC 时间戳）。
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
    now_unix: i64,
    offset_secs: i32,
    started_unix: i64,
    pref: DurationPref,
) -> Option<i64> {
    match pref {
        DurationPref::Indefinite => None,
        DurationPref::Hours { hours } => Some(started_unix + i64::from(hours) * 3600),
        DurationPref::UntilLocal { hour, minute } => {
            Some(next_until_unix_secs(now_unix, offset_secs, hour, minute))
        }
    }
}

pub fn format_duration_zh(secs: u64) -> String {
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
        assert_eq!(format_duration_zh(8), "8 秒");
        assert_eq!(format_duration_zh(60), "1 分钟");
        assert_eq!(format_duration_zh(90), "1 分 30 秒");
        assert_eq!(format_duration_zh(3600), "1 小时");
        assert_eq!(format_duration_zh(3720), "1 小时 2 分");
    }
}
