use axum::{
    extract::{State, Query},
    http::StatusCode,
    Json,
};
use chrono::{
    DateTime,
    Utc,
};
use eyre::WrapErr;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use sqlx::Row;
use crate::{
    AppState,
    dates::str2dt,
};

#[derive(Serialize, Debug)]
pub struct Consumption {
    timestamp: DateTime<Utc>,
    consumption: f32,
    generation: f32, // rename as export
    solar_generation: f32, // rename as generation
    import_rate: f32,
    export_rate: f32,
    cost: f32,
    sale: f32,
    charge: f32,
    discharge: f32,
    car_charge: f32,
}

#[derive(Deserialize, Debug)]
pub struct ConsumptionQuery {
    start: String,
    end: String,
    window: Option<String>,
}

#[axum::debug_handler]
pub async fn consumption(
    State(app): State<Arc<AppState>>,
    Query(query): Query<ConsumptionQuery>,
)
    -> Result<Json<Vec<Consumption>>, StatusCode>
{
    let ConsumptionQuery { start, end, window } = query;
    let mut data: Vec<Consumption> = Vec::new();

    let window: String = window.unwrap_or(String::from("30m"));

    ["1d", "1h", "30m"]
        .iter()
        .position(|i| *i == &window)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = sqlx::query(
        "WITH tariffs AS (
            SELECT
                i.valid_from,
                AVG(CASE WHEN pi.type='IMPORT' THEN i.value END) AS import_value,
                AVG(CASE WHEN pe.type='EXPORT' THEN e.value END) AS export_value

            FROM tariff_rates i

            JOIN tariff_rates e
              ON i.valid_from = e.valid_from

            JOIN products pi
              ON i.product_code = pi.product_code
             AND i.tariff_code = pi.tariff_code

            JOIN products pe
              ON e.product_code = pe.product_code
             AND e.tariff_code = pe.tariff_code

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

        grid AS (
            -- cost/sale are computed per raw half-hour (consumption * slot rate) and only summed afterwards.
            -- A bucketed window's rate ends up weighted by when consumption actually
            -- happened, not a plain average of the tariff schedule.
            SELECT
                MIN(t.valid_from) AS valid_from,
                CASE ?3
                    WHEN '1d' THEN date(t.valid_from)
                    WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', t.valid_from)
                    ELSE t.valid_from
                END AS slot,
                SUM(COALESCE(cs.consumption, 0.0)) AS consumption,
                SUM(COALESCE(cs.generation, 0.0)) AS generation,
                SUM(COALESCE(cs.consumption, 0.0) * t.import_value) AS cost,
                SUM(COALESCE(cs.generation, 0.0) * t.export_value) AS sale,

                -- Unweighted schedule price, used as the reported rate only
                -- when there was no consumption/generation to weight by.
                AVG(t.import_value) AS avg_import_value,
                AVG(t.export_value) AS avg_export_value

            FROM tariffs t
            LEFT JOIN consumption_sum cs ON cs.interval_start = t.valid_from
            GROUP BY slot
        ),

        charge_diff AS (
            SELECT
                MIN(c.start) AS start,
                CASE ?3
                    WHEN '1d' THEN date(c.start)
                    WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', c.start)
                    ELSE c.start
                END AS slot,

                SUM(c.charge) AS charge,
                SUM(c.discharge) AS discharge

            FROM charge c
            GROUP BY slot
        ),

        solar_sum AS (
            SELECT
                MIN(s.timestamp) AS timestamp,
                CASE ?3
                    WHEN '1d' THEN date(s.timestamp)
                    WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', s.timestamp)
                    ELSE s.timestamp
                END AS slot,
                SUM(value) AS solar

            FROM solar_generation s
            GROUP BY slot
        ),

        car_sum AS (
            SELECT
                MIN(c.timestamp) AS timestamp,
                CASE ?3
                    WHEN '1d' THEN date(c.timestamp)
                    WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', c.timestamp)
                    ELSE c.timestamp
                END AS slot,
                SUM(charge) AS car_charge

            FROM carcharge c
            GROUP BY slot
        )

        SELECT
            grid.slot,

            grid.consumption,
            grid.generation,

            COALESCE(grid.cost / NULLIF(grid.consumption, 0.0), grid.avg_import_value, 0.0) AS import_rate,
            COALESCE(grid.sale / NULLIF(grid.generation,  0.0), grid.avg_export_value, 0.0) AS export_rate,

            grid.cost,
            grid.sale,

            COALESCE(charge_diff.charge, 0.0),
            COALESCE(charge_diff.discharge, 0.0),
            COALESCE(solar_sum.solar, 0.0),
            COALESCE(car_sum.car_charge, 0.0)

        FROM grid
        LEFT JOIN charge_diff ON charge_diff.slot = grid.slot
        LEFT JOIN solar_sum ON solar_sum.slot = grid.slot
        LEFT JOIN car_sum ON car_sum.slot = grid.slot
        WHERE grid.valid_from >= ?1 AND grid.valid_from < ?2

        GROUP BY grid.slot
        ORDER BY grid.valid_from ASC"
    )
        .bind(&start)
        .bind(&end)
        .bind(&window)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| {
            log::error!("query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    for row in result {
        let consumption = Consumption {
            timestamp: str2dt(row.get(0))
                .wrap_err("timestamp")
                .map_err(|e| {
                    log::error!("timestamp: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?,
            consumption: row.get(1),
            generation: row.get(2),
            import_rate: row.get(3),
            export_rate: row.get(4),
            cost: row.get(5),
            sale: row.get(6),
            charge: row.get(7),
            discharge: row.get(8),
            solar_generation: row.get(9),
            car_charge: row.get(10),
        };
        data.push(consumption);
    }

    Ok(Json(data))
}

#[derive(Serialize, Debug)]
pub struct ConsumptionByTime {
    time_of_day: String,
    import_rate: f32,
    export_rate: f32,
    consumption: f32,
    generation: f32,
}

#[derive(Deserialize, Debug)]
pub struct ConsumptionByTimeQuery {
    start: String,
    end: String,
}

#[axum::debug_handler]
pub async fn consumption_by_time(
    State(app): State<Arc<AppState>>,
    Query(query): Query<ConsumptionByTimeQuery>,
)
    -> Result<Json<Vec<ConsumptionByTime>>, StatusCode>
{
    let ConsumptionByTimeQuery { start, end } = query;
    let mut data: Vec<ConsumptionByTime> = Vec::new();

    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = sqlx::query(
        // Both sources are collapsed to one row per half-hour before the
        // join. Overlapping tariff products would otherwise multiply every
        // SUM by the number of matching rate rows.
        "WITH tariffs AS (
            SELECT
                i.valid_from,
                AVG(CASE WHEN pi.type='IMPORT' THEN i.value END) AS import_value,
                AVG(CASE WHEN pe.type='EXPORT' THEN e.value END) AS export_value

            FROM tariff_rates i

            JOIN tariff_rates e
              ON i.valid_from = e.valid_from

            JOIN products pi
              ON i.product_code = pi.product_code
             AND i.tariff_code = pi.tariff_code

            JOIN products pe
              ON e.product_code = pe.product_code
             AND e.tariff_code = pe.tariff_code

           GROUP BY i.valid_from
        ),

        consumption_sum AS (
            SELECT
                interval_start,
                SUM(consumption) AS consumption,
                SUM(generation) AS generation

            FROM consumption
            GROUP BY interval_start
        )

        SELECT
            strftime('%H:%M', cs.interval_start) as t_30m,
            t.import_value as import_rate,
            t.export_value as export_rate,
            SUM(cs.consumption) as consumption,
            SUM(cs.generation) as generation

        FROM consumption_sum cs

        JOIN tariffs t
          ON t.valid_from = cs.interval_start

        WHERE
            cs.interval_start >= ? -- start
        AND cs.interval_start <  ? -- end
        AND t.import_value IS NOT NULL
        AND t.export_value IS NOT NULL

        GROUP BY t_30m, import_rate
        ORDER BY t_30m ASC, import_rate DESC;"
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
        let c = ConsumptionByTime {
            time_of_day: row.get(0),
            import_rate: row.get(1),
            export_rate: row.get(2),
            consumption: row.get(3),
            generation: row.get(4),
        };
        data.push(c);
    }

    Ok(Json(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::Error;

    use sqlx::pool::PoolConnection;
    use sqlx::sqlite::Sqlite;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DB_SEQ: AtomicUsize = AtomicUsize::new(0);

    /// A migrated, uniquely-named in-memory database plus the AppState over it.
    ///
    /// `cache=shared` so the pool's connections all see the same database --
    /// a bare `sqlite::memory:` is private per connection, and `AppState`
    /// recycles connections after 1s (`app.rs`), which would otherwise drop
    /// the seeded data mid-test. `_keepalive` holds the last reference open:
    /// a named shared-cache database is destroyed when its final connection
    /// closes.
    struct TestDb {
        app: Arc<AppState>,
        _keepalive: PoolConnection<Sqlite>,
    }

    impl TestDb {
        async fn new() -> Result<Self, Error> {
            let n = DB_SEQ.fetch_add(1, Ordering::Relaxed);
            let url = format!("sqlite:file:consumption_test_{n}?mode=memory&cache=shared");

            let app = Arc::new(AppState::connect(&url)?);
            let keepalive = app.acquire_sqlite().await?;

            let mut conn = app.acquire_sqlite().await?;
            crate::migrate::migrate(&mut conn).await?;
            sqlx::query(
                "INSERT INTO products (product_code, tariff_code, type, standing_charge)
                 VALUES ('P', 'IMP', 'IMPORT', 50.0), ('P', 'EXP', 'EXPORT', 0.0)"
            ).execute(&mut *conn).await?;
            drop(conn);

            Ok(Self { app, _keepalive: keepalive })
        }

        async fn conn(&self) -> Result<PoolConnection<Sqlite>, Error> {
            self.app.acquire_sqlite().await
        }

        async fn get(
            &self,
            start: &str,
            end: &str,
            window: Option<&str>,
        ) -> Result<Vec<Consumption>, StatusCode> {
            let query = ConsumptionQuery {
                start: start.to_string(),
                end: end.to_string(),
                window: window.map(String::from),
            };
            let Json(data) = consumption(State(self.app.clone()), Query(query)).await?;
            Ok(data)
        }

        /// Convenience for the common case: expect success.
        async fn ok(&self, start: &str, end: &str, window: &str) -> Vec<Consumption> {
            self.get(start, end, Some(window))
                .await
                .expect("consumption request should succeed")
        }
    }

    async fn insert_tariff_rate(
        conn: &mut sqlx::SqliteConnection,
        tariff_code: &str,
        valid_from: &str,
        value: f32,
    ) -> Result<(), Error> {
        insert_tariff_rate_for(conn, "P", tariff_code, valid_from, value).await
    }

    async fn insert_tariff_rate_for(
        conn: &mut sqlx::SqliteConnection,
        product_code: &str,
        tariff_code: &str,
        valid_from: &str,
        value: f32,
    ) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO tariff_rates (product_code, tariff_code, valid_from, value, daily_standing_charge)
             VALUES (?1, ?2, ?3, ?4, 50.0)"
        )
            .bind(product_code)
            .bind(tariff_code)
            .bind(valid_from)
            .bind(value)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    /// Both an IMPORT and an EXPORT rate for one half-hour, which is what the
    /// endpoint's tariff self-join requires before a slot appears at all.
    async fn insert_rates(
        conn: &mut sqlx::SqliteConnection,
        valid_from: &str,
        import: f32,
        export: f32,
    ) -> Result<(), Error> {
        insert_tariff_rate(conn, "IMP", valid_from, import).await?;
        insert_tariff_rate(conn, "EXP", valid_from, export).await?;
        Ok(())
    }

    async fn insert_consumption(
        conn: &mut sqlx::SqliteConnection,
        account: &str,
        interval_start: &str,
        consumption: f32,
        generation: f32,
    ) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO consumption (account, interval_start, interval_end, consumption, generation)
             VALUES (?1, ?2, ?2, ?3, ?4)"
        )
            .bind(account)
            .bind(interval_start)
            .bind(consumption)
            .bind(generation)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    async fn insert_charge(
        conn: &mut sqlx::SqliteConnection,
        inverter: &str,
        start: &str,
        charge: f32,
        discharge: f32,
    ) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO charge (inverter, start, end, charge, discharge)
             VALUES (?1, ?2, ?2, ?3, ?4)"
        )
            .bind(inverter)
            .bind(start)
            .bind(charge)
            .bind(discharge)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    async fn insert_solar(
        conn: &mut sqlx::SqliteConnection,
        serial: &str,
        timestamp: &str,
        value: f32,
    ) -> Result<(), Error> {
        sqlx::query("INSERT INTO solar_generation (serial, timestamp, value) VALUES (?1, ?2, ?3)")
            .bind(serial)
            .bind(timestamp)
            .bind(value)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    async fn insert_carcharge(
        conn: &mut sqlx::SqliteConnection,
        id: &str,
        timestamp: &str,
        charge: f32,
    ) -> Result<(), Error> {
        sqlx::query("INSERT INTO carcharge (id, timestamp, charge) VALUES (?1, ?2, ?3)")
            .bind(id)
            .bind(timestamp)
            .bind(charge)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    /// The 48 UTC half-hour slots of `date`, each with tariff rates and a
    /// consumption row. Returns the slot keys in order.
    async fn seed_half_hour_day(
        conn: &mut sqlx::SqliteConnection,
        date: &str,
        import: f32,
        export: f32,
        consumption: f32,
    ) -> Result<Vec<String>, Error> {
        let mut slots = Vec::new();
        for i in 0..48u32 {
            let minutes = i * 30;
            let slot = format!("{date}T{:02}:{:02}:00Z", minutes / 60, minutes % 60);
            insert_rates(conn, &slot, import, export).await?;
            insert_consumption(conn, "A", &slot, consumption, 0.0).await?;
            slots.push(slot);
        }
        Ok(slots)
    }

    fn sum_of(data: &[Consumption], f: impl Fn(&Consumption) -> f32) -> f32 {
        data.iter().map(f).sum()
    }

    /// Summed REALs come back with f32 rounding (0.3 + 0.4 == 0.70000005), so
    /// every value assertion here compares within a tolerance.
    #[track_caller]
    fn assert_close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 1e-4,
            "{what}: expected {expected}, got {actual}",
        );
    }

    // Every table/column the two handlers in this file query. If a collector
    // script renames a column, this fails here rather than at runtime.
    #[tokio::test]
    async fn migrate_provides_every_table_the_endpoint_queries() -> Result<(), Error> {
        use crate::migrate::{require_columns, SqlType::{Real, Text}};
        use sqlx::Connection;

        let mut conn = sqlx::SqliteConnection::connect("sqlite::memory:").await?;
        crate::migrate::migrate(&mut conn).await?;

        require_columns(&mut conn, "consumption", &[
            ("account", Text), ("interval_start", Text), ("interval_end", Text),
            ("consumption", Real), ("generation", Real),
        ]).await?;

        require_columns(&mut conn, "tariff_rates", &[
            ("product_code", Text), ("tariff_code", Text), ("valid_from", Text),
            ("value", Real), ("daily_standing_charge", Real),
        ]).await?;

        require_columns(&mut conn, "products", &[
            ("product_code", Text), ("tariff_code", Text), ("type", Text),
        ]).await?;

        require_columns(&mut conn, "solar_generation", &[
            ("serial", Text), ("timestamp", Text), ("value", Real),
        ]).await?;

        require_columns(&mut conn, "carcharge", &[
            ("id", Text), ("timestamp", Text), ("charge", Real),
        ]).await?;

        // Previously created ad hoc by the Python collectors, which now shell
        // out to the migrate-db binary instead -- so migrate() must own these.
        require_columns(&mut conn, "charge", &[
            ("inverter", Text), ("start", Text), ("end", Text),
            ("charge", Real), ("discharge", Real),
        ]).await?;

        require_columns(&mut conn, "versioning", &[
            ("key", Text), ("value", Text),
        ]).await?;

        Ok(())
    }

    #[tokio::test]
    async fn multi_inverter_aggregation_matches_true_sums() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        let slot = "2026-06-01T12:00:00Z";
        insert_rates(&mut conn, slot, 10.0, 5.0).await?;
        insert_consumption(&mut conn, "A", slot, 2.0, 0.5).await?;

        // Two real-world inverters contributing to the same slot -- the
        // regression this guards against summed grid/solar/car_charge once
        // per `charge` row instead of once per slot.
        insert_charge(&mut conn, "INV1", slot, 0.3, 0.0).await?;
        insert_charge(&mut conn, "INV2", slot, 0.4, 0.0).await?;

        insert_solar(&mut conn, "S1", slot, 1.0).await?;
        insert_carcharge(&mut conn, "C1", slot, 0.2).await?;

        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        assert_eq!(data.len(), 1, "expected exactly one slot");
        let row = &data[0];

        assert_close(row.consumption, 2.0, "grid consumption");
        assert_close(row.generation, 0.5, "grid generation");
        assert_close(row.charge, 0.7, "battery charge should sum across both inverters, not multiply the other sources");
        assert_close(row.discharge, 0.0, "battery discharge");
        assert_close(row.solar_generation, 1.0, "solar must not be fanned out by the 2 charge rows");
        assert_close(row.car_charge, 0.2, "car charge must not be fanned out by the 2 charge rows");
        assert_close(row.cost, 2.0 * 10.0, "cost");
        assert_close(row.sale, 0.5 * 5.0, "sale");

        // The invariant the backend must preserve: grid import can never be
        // lower than battery charge + car charge, net of solar generation.
        assert!(
            row.consumption >= row.charge + row.car_charge - row.solar_generation,
            "grid consumption {} should cover battery charge {} + car charge {} - solar {}",
            row.consumption, row.charge, row.car_charge, row.solar_generation,
        );

        Ok(())
    }

    #[tokio::test]
    async fn dst_fallback_day_returns_forty_eight_slots_and_tolerates_gaps() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // The UK DST fall-back happens at 2026-10-25T01:00:00Z, but storage
        // is UTC throughout, so this UTC day still has exactly 48 distinct
        // half-hour slots -- no duplicate/missing keys.
        //
        // No charge/solar_generation/carcharge rows at all -- every slot here
        // relies on the LEFT JOIN + COALESCE NULL-safety path.
        seed_half_hour_day(&mut conn, "2026-10-25", 10.0, 5.0, 0.1).await?;
        drop(conn);

        let data = db
            .get("2026-10-25T00:00:00Z", "2026-10-26T00:00:00Z", Some("30m"))
            .await
            .expect("request should succeed despite no battery/solar/car data");

        assert_eq!(data.len(), 48, "expected exactly 48 half-hour slots across the UTC day");

        for (i, row) in data.iter().enumerate() {
            assert_close(row.charge, 0.0, &format!("slot {i} missing charge data should coalesce to 0, not error"));
            assert_close(row.solar_generation, 0.0, &format!("slot {i} missing solar data should coalesce to 0, not error"));
            assert_close(row.car_charge, 0.0, &format!("slot {i} missing car charge data should coalesce to 0, not error"));
        }

        let mut timestamps: Vec<_> = data.iter().map(|r| r.timestamp).collect();
        let mut sorted = timestamps.clone();
        sorted.sort();
        timestamps.dedup();
        assert_eq!(timestamps.len(), 48, "slots must not be duplicated across the DST transition");
        assert_eq!(data.iter().map(|r| r.timestamp).collect::<Vec<_>>(), sorted, "slots must be strictly ordered");

        Ok(())
    }

    #[tokio::test]
    async fn bucketed_rate_is_weighted_by_consumption_not_averaged() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // Within one 1h bucket: all the actual consumption happens at a
        // negative rate, while a later half-hour with no consumption has a
        // large positive rate. A blind AVG(-10, 100) = 45 would flip the
        // bucket's rate/cost positive even though nothing was bought at
        // that price -- the weighted sum must stay negative.
        insert_rates(&mut conn, "2026-06-01T12:00:00Z", -10.0, 1.0).await?;
        insert_rates(&mut conn, "2026-06-01T12:30:00Z", 100.0, 1.0).await?;

        insert_consumption(&mut conn, "A", "2026-06-01T12:00:00Z", 5.0, 0.0).await?;
        insert_consumption(&mut conn, "A", "2026-06-01T12:30:00Z", 0.0, 0.0).await?;

        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T13:00:00Z", "1h").await;

        assert_eq!(data.len(), 1, "expected the two half-hours to collapse into one 1h bucket");
        let row = &data[0];

        assert_close(row.consumption, 5.0, "consumption");
        assert_close(row.cost, 5.0 * -10.0, "cost must be the sum of per-slot consumption * that slot's own rate");
        assert_close(row.import_rate, -10.0, "the weighted rate must reflect where consumption actually happened, not a blind average of the bucket's rates");
        assert!(row.cost < 0.0, "all consumption happened at a negative rate, so bucketed cost must stay negative");

        Ok(())
    }

    // ---- multi-device fan-out --------------------------------------------

    #[tokio::test]
    async fn fan_out_across_every_dimension_at_once() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // Worst case: every joined table has several rows for the same slot.
        // A flat join would return 3*2*2*2 = 24 duplicated rows and inflate
        // every SUM by the product of the other tables' row counts.
        let slot = "2026-06-01T12:00:00Z";
        insert_rates(&mut conn, slot, 10.0, 5.0).await?;

        insert_consumption(&mut conn, "ACC1", slot, 1.0, 0.25).await?;
        insert_consumption(&mut conn, "ACC2", slot, 2.0, 0.75).await?;

        insert_charge(&mut conn, "INV1", slot, 0.1, 0.01).await?;
        insert_charge(&mut conn, "INV2", slot, 0.2, 0.02).await?;
        insert_charge(&mut conn, "INV3", slot, 0.3, 0.03).await?;

        insert_solar(&mut conn, "S1", slot, 0.5).await?;
        insert_solar(&mut conn, "S2", slot, 1.5).await?;

        insert_carcharge(&mut conn, "C1", slot, 0.4).await?;
        insert_carcharge(&mut conn, "C2", slot, 0.6).await?;

        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;
        assert_eq!(data.len(), 1, "the slot must not be duplicated by the joins");
        let row = &data[0];

        assert_close(row.consumption, 3.0, "consumption summed over 2 accounts");
        assert_close(row.generation, 1.0, "generation summed over 2 accounts");
        assert_close(row.charge, 0.6, "charge summed over 3 inverters");
        assert_close(row.discharge, 0.06, "discharge summed over 3 inverters");
        assert_close(row.solar_generation, 2.0, "solar summed over 2 serials");
        assert_close(row.car_charge, 1.0, "car charge summed over 2 chargers");
        assert_close(row.cost, 3.0 * 10.0, "cost from the summed consumption, not a fanned-out one");
        assert_close(row.sale, 1.0 * 5.0, "sale from the summed generation, not a fanned-out one");

        Ok(())
    }

    #[tokio::test]
    async fn adding_another_inverter_does_not_change_grid_or_solar_totals() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        let slot = "2026-06-01T12:00:00Z";
        insert_rates(&mut conn, slot, 10.0, 5.0).await?;
        insert_consumption(&mut conn, "A", slot, 2.0, 0.5).await?;
        insert_solar(&mut conn, "S1", slot, 1.0).await?;
        insert_carcharge(&mut conn, "C1", slot, 0.2).await?;
        insert_charge(&mut conn, "INV1", slot, 0.1, 0.0).await?;
        insert_charge(&mut conn, "INV2", slot, 0.2, 0.0).await?;
        drop(conn);

        let before = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        // Commissioning another AIO must only move the battery figures.
        let mut conn = db.conn().await?;
        insert_charge(&mut conn, "INV3", slot, 0.3, 0.0).await?;
        drop(conn);

        let after = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        assert_eq!(before.len(), 1);
        assert_eq!(after.len(), 1, "a third inverter must not add rows");

        assert_close(after[0].consumption, before[0].consumption, "consumption must be unchanged by a new inverter");
        assert_close(after[0].generation, before[0].generation, "generation must be unchanged by a new inverter");
        assert_close(after[0].solar_generation, before[0].solar_generation, "solar must be unchanged by a new inverter");
        assert_close(after[0].car_charge, before[0].car_charge, "car charge must be unchanged by a new inverter");
        assert_close(after[0].cost, before[0].cost, "cost must be unchanged by a new inverter");

        assert_close(before[0].charge, 0.3, "two inverters");
        assert_close(after[0].charge, 0.6, "three inverters");

        Ok(())
    }

    // ---- tariff join -----------------------------------------------------

    #[tokio::test]
    async fn import_and_export_rates_are_not_swapped() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        let slot = "2026-06-01T12:00:00Z";
        insert_rates(&mut conn, slot, 21.0, 7.0).await?;
        insert_consumption(&mut conn, "A", slot, 1.0, 1.0).await?;
        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        assert_close(data[0].import_rate, 21.0, "import_rate must come from the IMPORT product");
        assert_close(data[0].export_rate, 7.0, "export_rate must come from the EXPORT product");
        assert_close(data[0].cost, 21.0, "cost is priced at the import rate");
        assert_close(data[0].sale, 7.0, "sale is priced at the export rate");

        Ok(())
    }

    #[tokio::test]
    async fn second_import_product_does_not_fan_out_or_null_the_rate() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // The live `products` table carries two IMPORT rows and one EXPORT.
        sqlx::query(
            "INSERT INTO products (product_code, tariff_code, type, standing_charge)
             VALUES ('P2', 'IMP2', 'IMPORT', 50.0)"
        ).execute(&mut *conn).await?;

        let slot = "2026-06-01T12:00:00Z";
        insert_rates(&mut conn, slot, 20.0, 5.0).await?;
        insert_tariff_rate_for(&mut conn, "P2", "IMP2", slot, 20.0).await?;
        insert_consumption(&mut conn, "A", slot, 1.0, 0.0).await?;
        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        assert_eq!(data.len(), 1, "a second IMPORT product must not duplicate the slot");
        assert_close(data[0].consumption, 1.0, "consumption must not be fanned out by the extra product");
        assert_close(data[0].import_rate, 20.0, "import_rate must resolve to the IMPORT price, not NULL");
        assert_close(data[0].cost, 20.0, "cost must not be multiplied by the number of IMPORT products");

        Ok(())
    }

    #[tokio::test]
    async fn mixed_timestamp_suffix_does_not_silently_merge_slots() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // Collectors must write `...Z`. A `+00:00`-suffixed row denotes the
        // same instant but joins as a distinct key, which is the hazard
        // queries/drop-dst-suffix.sql exists to clean up. Pinned here so the
        // behaviour is visible rather than a silent mis-total.
        insert_rates(&mut conn, "2026-06-01T12:00:00Z", 10.0, 5.0).await?;
        insert_rates(&mut conn, "2026-06-01T12:00:00+00:00", 10.0, 5.0).await?;

        insert_consumption(&mut conn, "A", "2026-06-01T12:00:00Z", 1.0, 0.0).await?;
        insert_consumption(&mut conn, "A", "2026-06-01T12:00:00+00:00", 99.0, 0.0).await?;
        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        assert_eq!(data.len(), 1, "only the Z-suffixed slot falls inside the lexicographic range");
        assert_close(
            data[0].consumption, 1.0,
            "the +00:00 row must not merge into the Z slot -- mismatched formats stay separate",
        );

        Ok(())
    }

    // ---- window bucketing ------------------------------------------------

    #[tokio::test]
    async fn day_window_returns_parseable_midnight_timestamp() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;
        seed_half_hour_day(&mut conn, "2026-10-25", 10.0, 5.0, 0.1).await?;
        drop(conn);

        // Guards the strftime('%T'...) typo, which produced an unparseable
        // slot label ("13:30:00-10-25T00:00:00Z") and a 500 from str2dt.
        let data = db.ok("2026-10-25T00:00:00Z", "2026-10-26T00:00:00Z", "1d").await;

        assert_eq!(data.len(), 1, "a whole day collapses to a single 1d bucket");
        assert_eq!(
            data[0].timestamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-10-25T00:00:00Z",
            "the 1d bucket must be labelled with its own UTC midnight",
        );

        Ok(())
    }

    #[tokio::test]
    async fn totals_are_conserved_across_all_three_windows() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        let slots = seed_half_hour_day(&mut conn, "2026-10-25", 10.0, 5.0, 0.1).await?;
        // Vary the other sources so conservation is a real check, not 0 == 0.
        for (i, slot) in slots.iter().enumerate() {
            insert_charge(&mut conn, "INV1", slot, 0.02, 0.01).await?;
            insert_charge(&mut conn, "INV2", slot, 0.03, 0.0).await?;
            if i % 2 == 0 {
                insert_solar(&mut conn, "S1", slot, 0.04).await?;
                insert_carcharge(&mut conn, "C1", slot, 0.05).await?;
            }
        }
        drop(conn);

        let half = db.ok("2026-10-25T00:00:00Z", "2026-10-26T00:00:00Z", "30m").await;
        let hour = db.ok("2026-10-25T00:00:00Z", "2026-10-26T00:00:00Z", "1h").await;
        let day = db.ok("2026-10-25T00:00:00Z", "2026-10-26T00:00:00Z", "1d").await;

        assert_eq!((half.len(), hour.len(), day.len()), (48, 24, 1), "bucket counts for 30m/1h/1d");

        // Re-bucketing may only regroup energy, never create or destroy it.
        let fields: [(&str, fn(&Consumption) -> f32); 8] = [
            ("consumption", |c| c.consumption),
            ("generation", |c| c.generation),
            ("cost", |c| c.cost),
            ("sale", |c| c.sale),
            ("charge", |c| c.charge),
            ("discharge", |c| c.discharge),
            ("solar_generation", |c| c.solar_generation),
            ("car_charge", |c| c.car_charge),
        ];

        for (name, f) in fields {
            let expected = sum_of(&half, f);
            assert_close(sum_of(&hour, f), expected, &format!("{name} total must survive 1h bucketing"));
            assert_close(sum_of(&day, f), expected, &format!("{name} total must survive 1d bucketing"));
        }

        Ok(())
    }

    #[tokio::test]
    async fn invalid_window_is_rejected() -> Result<(), Error> {
        let db = TestDb::new().await?;

        let status = db
            .get("2026-10-25T00:00:00Z", "2026-10-26T00:00:00Z", Some("15m"))
            .await
            .expect_err("an unsupported window must be rejected");

        assert_eq!(status, StatusCode::BAD_REQUEST);
        Ok(())
    }

    #[tokio::test]
    async fn omitted_window_defaults_to_thirty_minutes() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;
        seed_half_hour_day(&mut conn, "2026-10-25", 10.0, 5.0, 0.1).await?;
        drop(conn);

        let data = db
            .get("2026-10-25T00:00:00Z", "2026-10-26T00:00:00Z", None)
            .await
            .expect("request without an explicit window should succeed");

        assert_eq!(data.len(), 48, "the default window is 30m");
        Ok(())
    }

    // ---- weighted rates --------------------------------------------------

    #[tokio::test]
    async fn bucketed_export_rate_is_weighted_by_generation() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // Mirror of the import case: all export happens in the low-price
        // half-hour, so the bucket's export_rate must be the low price.
        insert_rates(&mut conn, "2026-06-01T12:00:00Z", 10.0, 2.0).await?;
        insert_rates(&mut conn, "2026-06-01T12:30:00Z", 10.0, 50.0).await?;

        insert_consumption(&mut conn, "A", "2026-06-01T12:00:00Z", 0.0, 4.0).await?;
        insert_consumption(&mut conn, "A", "2026-06-01T12:30:00Z", 0.0, 0.0).await?;
        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T13:00:00Z", "1h").await;

        assert_eq!(data.len(), 1);
        assert_close(data[0].generation, 4.0, "generation");
        assert_close(data[0].sale, 4.0 * 2.0, "sale must price each slot's generation at that slot's own rate");
        assert_close(data[0].export_rate, 2.0, "export_rate must be weighted by generation, not averaged over the schedule");

        Ok(())
    }

    #[tokio::test]
    async fn all_negative_price_consumption_keeps_day_aggregate_negative() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // The 2026-10-25 shape in miniature: consumption lands entirely in
        // cheap overnight slots at negative prices, while the (idle) rest of
        // the day is expensive. Averaging the schedule flips the day's sign.
        for i in 0..48u32 {
            let minutes = i * 30;
            let slot = format!("2026-10-25T{:02}:{:02}:00Z", minutes / 60, minutes % 60);
            let overnight = i < 10;
            let import = if overnight { -5.0 } else { 30.0 };
            insert_rates(&mut conn, &slot, import, 5.0).await?;
            insert_consumption(&mut conn, "A", &slot, if overnight { 2.0 } else { 0.0 }, 0.0).await?;
        }
        drop(conn);

        let day = db.ok("2026-10-25T00:00:00Z", "2026-10-26T00:00:00Z", "1d").await;
        assert_eq!(day.len(), 1);

        assert_close(day[0].consumption, 20.0, "all 10 overnight slots at 2.0 kWh");
        assert_close(day[0].cost, 20.0 * -5.0, "the day's cost is the sum of per-slot costs");
        assert!(day[0].cost < 0.0, "a day consumed entirely at negative prices must report negative cost");
        assert!(day[0].import_rate < 0.0, "and a negative effective import rate");
        assert_close(day[0].import_rate, -5.0, "the effective rate is the price actually paid");

        // The 1d aggregate must agree with the per-slot truth.
        let half = db.ok("2026-10-25T00:00:00Z", "2026-10-26T00:00:00Z", "30m").await;
        assert_close(day[0].cost, sum_of(&half, |c| c.cost), "1d cost must reconcile with the 30m slots");

        Ok(())
    }

    #[tokio::test]
    async fn zero_consumption_slot_reports_schedule_rate() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // Nothing was imported or exported, so there is no weight to apply.
        // The frontend still plots import_rate as the price line, so the slot
        // must report its scheduled price rather than a misleading 0.
        let slot = "2026-06-01T12:00:00Z";
        insert_rates(&mut conn, slot, 24.5, 6.5).await?;
        insert_consumption(&mut conn, "A", slot, 0.0, 0.0).await?;
        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        assert_eq!(data.len(), 1);
        assert_close(data[0].consumption, 0.0, "consumption");
        assert_close(data[0].cost, 0.0, "no consumption means no cost");
        assert_close(data[0].import_rate, 24.5, "a zero-consumption slot falls back to its scheduled import price");
        assert_close(data[0].export_rate, 6.5, "a zero-generation slot falls back to its scheduled export price");

        Ok(())
    }

    #[tokio::test]
    async fn slot_with_no_consumption_row_at_all_reports_schedule_rate() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // Same as above, but the consumption row is absent rather than zero
        // -- the LEFT JOIN path. Tariffs are published ahead of meter reads,
        // so this is the normal shape for the rest of the current day.
        let slot = "2026-06-01T12:00:00Z";
        insert_rates(&mut conn, slot, 18.0, 4.0).await?;
        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        assert_eq!(data.len(), 1, "a published tariff slot appears even with no meter data yet");
        assert_close(data[0].consumption, 0.0, "missing consumption coalesces to 0");
        assert_close(data[0].import_rate, 18.0, "the scheduled price is still reported");

        Ok(())
    }

    // ---- range filtering -------------------------------------------------

    #[tokio::test]
    async fn range_is_start_inclusive_end_exclusive() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        for (slot, kwh) in [
            ("2026-06-01T11:30:00Z", 1.0),
            ("2026-06-01T12:00:00Z", 2.0),
            ("2026-06-01T12:30:00Z", 4.0),
        ] {
            insert_rates(&mut conn, slot, 10.0, 5.0).await?;
            insert_consumption(&mut conn, "A", slot, kwh, 0.0).await?;
        }
        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        assert_eq!(data.len(), 1, "start is inclusive and end is exclusive");
        assert_close(data[0].consumption, 2.0, "only the slot at `start` is returned");

        Ok(())
    }

    #[tokio::test]
    async fn empty_range_returns_no_rows() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;
        insert_rates(&mut conn, "2026-06-01T12:00:00Z", 10.0, 5.0).await?;
        insert_consumption(&mut conn, "A", "2026-06-01T12:00:00Z", 1.0, 0.0).await?;
        drop(conn);

        // A window with no data is an empty result, not an error.
        let data = db.ok("2027-01-01T00:00:00Z", "2027-01-02T00:00:00Z", "30m").await;
        assert!(data.is_empty(), "a range with no data returns no rows");

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:00:00Z", "30m").await;
        assert!(data.is_empty(), "an empty start==end range returns no rows");

        Ok(())
    }

    #[tokio::test]
    async fn rows_outside_the_range_are_not_summed_in() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // Battery/solar/car rows just outside the window must not leak into
        // the in-range bucket -- their CTEs are grouped before the filter.
        for slot in ["2026-06-01T11:30:00Z", "2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z"] {
            insert_rates(&mut conn, slot, 10.0, 5.0).await?;
            insert_consumption(&mut conn, "A", slot, 1.0, 0.0).await?;
            insert_charge(&mut conn, "INV1", slot, 0.5, 0.25).await?;
            insert_solar(&mut conn, "S1", slot, 0.75).await?;
            insert_carcharge(&mut conn, "C1", slot, 0.125).await?;
        }
        drop(conn);

        let data = db.ok("2026-06-01T12:00:00Z", "2026-06-01T12:30:00Z", "30m").await;

        assert_eq!(data.len(), 1);
        assert_close(data[0].consumption, 1.0, "consumption from the in-range slot only");
        assert_close(data[0].charge, 0.5, "charge from the in-range slot only");
        assert_close(data[0].discharge, 0.25, "discharge from the in-range slot only");
        assert_close(data[0].solar_generation, 0.75, "solar from the in-range slot only");
        assert_close(data[0].car_charge, 0.125, "car charge from the in-range slot only");

        Ok(())
    }

    // ---- NULL / gap safety -----------------------------------------------

    #[tokio::test]
    async fn partial_gaps_within_a_bucket_coalesce_to_zero() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // One half-hour has solar/car/battery rows, the other has none. The
        // 1h bucket must total the present half-hour. This also pins the
        // COALESCE(..., 0.0) literal: an INTEGER `0` makes sqlx fail with
        // "f32 ... not compatible with SQL type INTEGER".
        insert_rates(&mut conn, "2026-06-01T12:00:00Z", 10.0, 5.0).await?;
        insert_rates(&mut conn, "2026-06-01T12:30:00Z", 10.0, 5.0).await?;

        insert_consumption(&mut conn, "A", "2026-06-01T12:00:00Z", 1.0, 0.0).await?;
        insert_consumption(&mut conn, "A", "2026-06-01T12:30:00Z", 1.0, 0.0).await?;

        insert_charge(&mut conn, "INV1", "2026-06-01T12:00:00Z", 0.4, 0.2).await?;
        insert_solar(&mut conn, "S1", "2026-06-01T12:00:00Z", 0.8).await?;
        insert_carcharge(&mut conn, "C1", "2026-06-01T12:00:00Z", 0.6).await?;
        drop(conn);

        let hour = db.ok("2026-06-01T12:00:00Z", "2026-06-01T13:00:00Z", "1h").await;
        assert_eq!(hour.len(), 1);
        assert_close(hour[0].consumption, 2.0, "both half-hours have consumption");
        assert_close(hour[0].charge, 0.4, "the gapped half-hour contributes 0, not NULL");
        assert_close(hour[0].discharge, 0.2, "the gapped half-hour contributes 0, not NULL");
        assert_close(hour[0].solar_generation, 0.8, "the gapped half-hour contributes 0, not NULL");
        assert_close(hour[0].car_charge, 0.6, "the gapped half-hour contributes 0, not NULL");

        // And the gapped slot itself reports zeros at 30m granularity.
        let half = db.ok("2026-06-01T12:30:00Z", "2026-06-01T13:00:00Z", "30m").await;
        assert_eq!(half.len(), 1);
        assert_close(half[0].charge, 0.0, "a slot with no charge row reports 0");
        assert_close(half[0].solar_generation, 0.0, "a slot with no solar row reports 0");
        assert_close(half[0].car_charge, 0.0, "a slot with no carcharge row reports 0");

        Ok(())
    }

    #[tokio::test]
    async fn spring_forward_day_also_has_forty_eight_utc_slots() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // 2026-03-29 is the 23-hour UK local day (GMT->BST). Bucketing is
        // UTC, so it is still 48 half-hours -- DST invariance in the other
        // direction from the fall-back test.
        seed_half_hour_day(&mut conn, "2026-03-29", 10.0, 5.0, 0.1).await?;
        drop(conn);

        let half = db.ok("2026-03-29T00:00:00Z", "2026-03-30T00:00:00Z", "30m").await;
        let hour = db.ok("2026-03-29T00:00:00Z", "2026-03-30T00:00:00Z", "1h").await;

        assert_eq!(half.len(), 48, "a spring-forward UTC day is still 48 half-hours");
        assert_eq!(hour.len(), 24, "and still 24 UTC hours");

        // Timestamps must round-trip as UTC with no local-time shift.
        assert_eq!(
            half[0].timestamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-03-29T00:00:00Z",
        );
        assert_eq!(
            half[47].timestamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-03-29T23:30:00Z",
        );

        Ok(())
    }

    // ---- sibling handler -------------------------------------------------

    #[tokio::test]
    async fn consumption_by_time_sums_across_accounts_without_fan_out() -> Result<(), Error> {
        let db = TestDb::new().await?;
        let mut conn = db.conn().await?;

        // Same tariff double-join as the main handler, so give it two
        // accounts and a second IMPORT product to fan out against.
        sqlx::query(
            "INSERT INTO products (product_code, tariff_code, type, standing_charge)
             VALUES ('P2', 'IMP2', 'IMPORT', 50.0)"
        ).execute(&mut *conn).await?;

        let slot = "2026-06-01T12:00:00Z";
        insert_rates(&mut conn, slot, 10.0, 5.0).await?;
        insert_tariff_rate_for(&mut conn, "P2", "IMP2", slot, 10.0).await?;
        insert_consumption(&mut conn, "ACC1", slot, 1.0, 0.25).await?;
        insert_consumption(&mut conn, "ACC2", slot, 2.0, 0.75).await?;
        drop(conn);

        let query = ConsumptionByTimeQuery {
            start: "2026-06-01T12:00:00Z".to_string(),
            end: "2026-06-01T12:30:00Z".to_string(),
        };
        let Json(data) = consumption_by_time(State(db.app.clone()), Query(query))
            .await
            .expect("consumption-by-time request should succeed");

        assert_eq!(data.len(), 1, "one time-of-day bucket");
        assert_eq!(data[0].time_of_day, "12:00");
        assert_close(data[0].consumption, 3.0, "consumption summed over both accounts, not fanned out");
        assert_close(data[0].generation, 1.0, "generation summed over both accounts, not fanned out");
        assert_close(data[0].import_rate, 10.0, "import rate from the IMPORT product");
        assert_close(data[0].export_rate, 5.0, "export rate from the EXPORT product");

        Ok(())
    }
}
