use eyre::{Error, Context};
use chrono::{
    DateTime,
    NaiveDate,
    NaiveDateTime,
    Timelike,
    Utc,
};

pub fn dt2str(dt: DateTime<Utc>) -> String {
    dt
        .with_second(0).unwrap()
        .with_nanosecond(0).unwrap()
        .format("%Y-%m-%dT%H:%M:00Z")
        .to_string()
}

pub fn str2dt(s: &str) -> Result<DateTime<Utc>, Error> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
        return Ok(dt.into());
    };

    if let Ok(ndt) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%MZ") {
        let dt: DateTime<Utc> = ndt.and_utc();
        return Ok(dt);
    };

    if let Ok(dt) = DateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M%:z") {
        return Ok(dt.into());
    };

    let dt: DateTime<Utc> = NaiveDate::parse_from_str(&s, "%Y-%m-%d")
        .map(|dt| {
            dt
            .and_hms_opt(0, 0, 0).unwrap() // always valid, hardcoded
            .and_utc()
        })
        .wrap_err_with(|| format!("original s: '{s}'"))?;
    Ok(dt.into())
}

#[test]
fn test_str2dt() {
    let cases = [
        "2026-05-06",
        "2026-05-06T21:30Z",
        "2026-05-06T21:30:00Z",
        "2026-05-06T21:30+01:00",
    ];
    for s in cases {
        str2dt(s).expect("valid datetime");
    }
}

#[test]
fn test_dt2str() {
    let cases = [
        ("2026-05-06", "2026-05-06T00:00:00Z"),
        ("2026-05-06T21:30Z", "2026-05-06T21:30:00Z"),
        ("2026-05-06T21:30:00Z", "2026-05-06T21:30:00Z"),
        ("2026-05-06T21:30+01:00", "2026-05-06T20:30:00Z"),
    ];
    for (input, expected) in cases {
        let dt = str2dt(input).expect("valid datetime");
        assert_eq!(dt2str(dt), expected);
    }
}
