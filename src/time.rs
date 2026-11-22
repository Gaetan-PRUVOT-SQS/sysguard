use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_timestamp() -> String {
    timestamp(SystemTime::now())
}

pub fn now_timestamp_secondes() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(duration.as_secs()).unwrap_or(0);
    let (year, month, day, hour, min, sec) = decomposer(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

pub fn timestamp(t: SystemTime) -> String {
    let duration = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = i64::try_from(duration.as_secs()).unwrap_or(0);
    let millis = duration.subsec_millis();
    let (year, month, day, hour, min, sec) = decomposer(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{millis:03}Z")
}

fn decomposer(secs: i64) -> (i64, u32, u32, i64, i64, i64) {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let sec = secs_of_day % 60;
    (year, month, day, hour, min, sec)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year_of_era = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 {
        year_of_era + 1
    } else {
        year_of_era
    };
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn epoch() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn instant_known() {
        let t = UNIX_EPOCH + Duration::new(1_795_290_839, 427_000_000);
        assert_eq!(timestamp(t), "2026-11-21T19:53:59.427Z");
    }

    #[test]
    fn leap_day() {
        let t = UNIX_EPOCH + Duration::new(1_709_164_800, 0);
        assert_eq!(timestamp(t), "2024-02-29T00:00:00.000Z");
    }

    #[test]
    fn secondes_sans_millis() {
        let t = UNIX_EPOCH + Duration::new(1_795_290_839, 427_000_000);
        let duration = t.duration_since(UNIX_EPOCH).expect("durée");
        let secs = i64::try_from(duration.as_secs()).expect("secs");
        let (year, month, day, hour, min, sec) = decomposer(secs);
        let texte = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z");
        assert_eq!(texte, "2026-11-21T19:53:59Z");
    }

    #[test]
    fn leap_day_of_century() {
        let t = UNIX_EPOCH + Duration::new(951_782_400, 0);
        assert_eq!(timestamp(t), "2000-02-29T00:00:00.000Z");
        let t = UNIX_EPOCH + Duration::new(951_868_800, 0);
        assert_eq!(timestamp(t), "2000-03-01T00:00:00.000Z");
    }

    #[test]
    fn before_epoch_clamped() {
        let t = UNIX_EPOCH - Duration::new(100, 0);
        assert_eq!(timestamp(t), "1970-01-01T00:00:00.000Z");
    }
}
