use eyre::{Error, Context};
use chrono::{
    DateTime,
    NaiveDate,
    NaiveDateTime,
    Utc,
};

pub fn dt2str(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%MZ").to_string()
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
    str2dt("2026-05-06").expect("valid date");
    str2dt("2026-05-06T21:30Z").expect("valid datetime");
    str2dt("2026-05-06T21:30+01:00").expect("valid datetime");
}
