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

#[derive(Serialize, Debug)]
pub struct PriceDistribution {
    import_rate: f32,
    export_rate: f32,
    consumption: f32,
    generation: f32,
    slots: u32,
    cost: f32,
    sale: f32,
}

#[derive(Deserialize, Debug)]
pub struct PriceDistributionQuery {
    start: String,
    end: String,
}

pub async fn price_distribution(
    State(app): State<Arc<AppState>>,
    Query(query): Query<PriceDistributionQuery>,
)
    -> Result<Json<Vec<PriceDistribution>>, StatusCode>
{
    let PriceDistributionQuery { start, end } = query;
    let mut data: Vec<PriceDistribution> = Vec::new();

    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let result = sqlx::query(
        "SELECT
            import.value as import_rate,
            export.value as export_rate,
            c.consumption as consumption,
            c.generation as generation,
            1 as slots,
            (import.value * c.consumption) as cost,
            (export.value * c.generation) as sale
        FROM consumption as c

        LEFT JOIN tariff_rates as import

        LEFT JOIN tariff_rates as export
        ON
            c.interval_start = import.valid_from
        AND c.interval_start = export.valid_from

        JOIN products as pimport
        ON
            import.product_code = pimport.product_code
        AND import.tariff_code = pimport.tariff_code

        JOIN products as pexport
        ON
            export.product_code = pexport.product_code
        AND export.tariff_code = pexport.tariff_code

        WHERE
            c.interval_start >= ? -- start
        AND c.interval_start <  ? -- end
        AND pimport.type = 'IMPORT'
        AND pexport.type = 'EXPORT';"
    )
        .bind(&start)
        .bind(&end)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| {
            log::error!("query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    for row in result {
        let pd = PriceDistribution {
            import_rate: row.get(0),
            export_rate: row.get(1),
            consumption: row.get(2),
            generation: row.get(3),
            slots: row.get(4),
            cost: row.get(5),
            sale: row.get(6),
        };
        data.push(pd);
    }

    Ok(Json(data))
}
