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
    import_incentive: f32, // p/kWh, <= 0
    export_incentive: f32, // p/kWh, >= 0
    cost_incentive: f32,   // p, <= 0
    sale_incentive: f32,   // p, >= 0
}

#[derive(Deserialize, Debug)]
pub struct PriceDistributionQuery {
    pub(crate) start: String,
    pub(crate) end: String,
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
        "WITH

        tariffs AS (
            SELECT
                i.valid_from,
                AVG(CASE WHEN i.type='IMPORT' THEN i.value END) AS import_value,
                AVG(CASE WHEN e.type='EXPORT' THEN e.value END) AS export_value

            -- Only the tariff an agreement covers is billed for each slot.
            FROM billed_tariff_rates i

            JOIN billed_tariff_rates e
              ON i.valid_from = e.valid_from

           GROUP BY i.valid_from
        ),

        consumption_sum AS (
            SELECT
                interval_start,
                SUM(consumption) AS consumption,
                SUM(generation) AS generation

            FROM consumption
            GROUP BY interval_start
        ),

        promo_slots AS (
            SELECT
                t.valid_from,
                t.import_value,
                p.event_type,
                p.reward_octopoints / 8.0 AS reward_pence,
                p.free_import_kwh_cap,
                COALESCE(cs.consumption, 0.0) AS consumption,
                COALESCE(cs.generation, 0.0) AS generation,
                SUM(COALESCE(cs.consumption, 0.0)) OVER event AS event_consumption,
                SUM(COALESCE(cs.generation, 0.0)) OVER event AS event_generation,
                COUNT(*) OVER event AS event_slots

            FROM promotion_events p
            JOIN tariffs t
              ON t.valid_from >= p.start_at
             AND t.valid_from <  p.end_at
            LEFT JOIN consumption_sum cs ON cs.interval_start = t.valid_from

            WINDOW event AS (PARTITION BY p.account, p.event_id)
        ),

        slot_incentives AS (
            SELECT
                valid_from,

                SUM(CASE event_type

                    -- Free import, capped per event; the cap is shared
                    -- across the event's slots in proportion to usage.
                    WHEN 'WEEKEND_HAPPY_HOUR' THEN
                        -consumption * import_value
                        * MIN(1.0, COALESCE(free_import_kwh_cap / NULLIF(event_consumption, 0.0), 1.0))

                    -- Power Up: reward for importing more, credited against import.
                    WHEN 'TURN_UP' THEN
                        -COALESCE(reward_pence, 0.0)
                        * COALESCE(consumption / NULLIF(event_consumption, 0.0), 1.0 / event_slots)

                    ELSE 0.0
                END) AS cost_incentive,

                SUM(CASE event_type

                    -- Power Down: reward treated as a boost to the export price.
                    WHEN 'TURN_DOWN' THEN
                        COALESCE(reward_pence, 0.0)
                        * COALESCE(generation / NULLIF(event_generation, 0.0), 1.0 / event_slots)

                    ELSE 0.0
                END) AS sale_incentive,

                SUM(CASE event_type
                    WHEN 'WEEKEND_HAPPY_HOUR' THEN -import_value
                    ELSE 0.0
                END) AS schedule_import_incentive

            FROM promo_slots
            GROUP BY valid_from
        ),

        -- One row per half-hour with meter data: the tariff price plus any
        -- Saving Sessions adjustment, kept as separate columns.
        slot_prices AS (
            SELECT
                cs.interval_start AS valid_from,
                cs.consumption,
                cs.generation,
                t.import_value,
                t.export_value,
                COALESCE(si.cost_incentive, 0.0) AS cost_incentive,
                COALESCE(si.sale_incentive, 0.0) AS sale_incentive,
                COALESCE(si.cost_incentive / NULLIF(cs.consumption, 0.0), si.schedule_import_incentive, 0.0) AS import_incentive,
                COALESCE(si.sale_incentive / NULLIF(cs.generation, 0.0), 0.0) AS export_incentive

            FROM consumption_sum cs
            JOIN tariffs t ON t.valid_from = cs.interval_start
            LEFT JOIN slot_incentives si ON si.valid_from = cs.interval_start
        )

        SELECT
            import_value AS import_rate,
            export_value AS export_rate,
            consumption,
            generation,
            1 AS slots,
            import_value * consumption AS cost,
            export_value * generation AS sale,
            import_incentive,
            export_incentive,
            cost_incentive,
            sale_incentive

        FROM slot_prices

        WHERE
            valid_from >= ? -- start
        AND valid_from <  ? -- end
        AND import_value IS NOT NULL
        AND export_value IS NOT NULL;"
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
            import_incentive: row.get(7),
            export_incentive: row.get(8),
            cost_incentive: row.get(9),
            sale_incentive: row.get(10),
        };
        data.push(pd);
    }

    Ok(Json(data))
}
