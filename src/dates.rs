use eyre::{Error, Context, OptionExt};
use chrono::{
    DateTime,
    FixedOffset,
    NaiveDate,
    NaiveDateTime,
    Timelike,
    Utc,
};
use std::str::FromStr;

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

    if let Ok(dt) = parse_from_foxess(&s) {
        return Ok(dt);
    }

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

fn parse_from_foxess(s: &str) -> Result<DateTime<Utc>, Error> {
    let (ndt, remainder) = NaiveDateTime::parse_and_remainder(s, "%Y-%m-%d %H:%M:%S")?;

    let idx: usize = remainder.find('+').ok_or_eyre("tz easting")?;
    let (_, remainder) = remainder.split_at(idx);

    // Formatted as `%z`
    let tz = FixedOffset::from_str(&remainder)?;

    let dt: DateTime<Utc> = ndt
        // https://docs.rs/chrono/latest/chrono/offset/enum.LocalResult.html
        .and_local_timezone(tz).latest().ok_or_eyre("no tz")?
        .to_utc();

    Ok(dt)
}


#[test]
fn test_str2dt() {
    let cases = [
        "2026-05-06",
        "2026-05-06T21:30Z",
        "2026-05-06T21:30:00Z",
        "2026-05-06T21:30+01:00",
        "2026-05-23 21:03:29 BST+0100",
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
