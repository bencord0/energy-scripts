use axum::{
    extract::{State, Query},
    http::StatusCode,
    Json,
};
use eyre::WrapErr;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use sqlx::Row;
use crate::AppState;

#[derive(Serialize,Debug)]
pub struct StandingCharge {
    day: String,
    daily_standing_charge: f32,
    slots: u32,
}

#[derive(Deserialize,Debug)]
pub struct StandingChargeQuery {
    start: String,
    end: String,
    r#type: String,
}

pub async fn standing_charge(
    State(app): State<Arc<AppState>>,
    Query(query): Query<StandingChargeQuery>,
)
    -> Result<Json<Vec<StandingCharge>>, StatusCode>
{
    let StandingChargeQuery { start, end, r#type } = query;

    let mut data: Vec<StandingCharge> = Vec::new();
    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = sqlx::query(
        "SELECT
            date(r.valid_from) as day,
            AVG(r.daily_standing_charge),
            COUNT(*) as slots
        FROM tariff_rates as r
        JOIN products as p
          ON r.product_code = p.product_code
         AND r.tariff_code = p.tariff_code

        WHERE r.valid_from >= ? -- start
          AND r.valid_from <  ? -- end
          AND p.type = ?        -- type

        GROUP BY day
        ORDER BY day ASC"
    )
        .bind(&start)
        .bind(&end)
        .bind(&r#type)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| {
            log::error!("Fetch version: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    for row in result {
        let s = StandingCharge {
            day: row.get(0),
            daily_standing_charge: row.get(1),
            slots: row.get(2),
        };
        data.push(s);
    }

    Ok(Json(data))
}
