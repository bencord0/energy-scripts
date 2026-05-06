import { default as sqlite3WasmInit } from '/js/sqlite-wasm-3510100/jswasm/sqlite3.mjs';

let sqlite3Promise = null;
async function getSqlite3() {
    if (!sqlite3Promise) {
        sqlite3Promise = sqlite3WasmInit();
    }
    return sqlite3Promise;
}

export async function getDataBuffer() {
    const cacheName = 'octopus-data-v1';
    const url = '/data/power.sqlite3';

    // Speed up first page load if we have visited the site before
    if ('caches' in window) {
        const cache = await caches.open(cacheName);
        const cachedResponse = await cache.match(url);

        if (cachedResponse) {
            const buffer = await cachedResponse.arrayBuffer();

            // Check version
            try {
                const sqlite3 = await getSqlite3();
                const db = new sqlite3.oo1.DB();
                const rc = sqlite3.capi.sqlite3_deserialize(
                    db.pointer,
                    'main',
                    sqlite3.wasm.allocFromTypedArray(buffer),
                    buffer.byteLength,
                    buffer.byteLength,
                    sqlite3.capi.SQLITE_DESERIALIZE_FREEONCLOSE,
                );
                db.checkRc(rc);

                let dbVersion = null;
                db.exec({
                    sql: "SELECT value FROM versioning WHERE key = 'version'",
                    callback: (row) => { dbVersion = row[0]; }
                });
                db.close();

                let DB_VERSION = await (await fetch("/version")).text();

                if (dbVersion === DB_VERSION) {
                    console.log('Using cached database (version ' + dbVersion + ')');
                    return buffer;
                } else {
                    console.warn('Database version mismatch. Expected: ' + DB_VERSION + ', Found: ' + dbVersion);
                }
            } catch (e) {
                console.warn('Could not check database version (might be old schema):', e);
            }
        }

        console.log('Fetching database and caching...');
        const response = await fetch(url);
        await cache.put(url, response.clone());
        return await response.arrayBuffer();
    }

    return await fetch(url).then(res => res.arrayBuffer());
}

export async function initDatabase() {
    const [sqlite3, dataBuffer] = await Promise.all([
        getSqlite3(),
        getDataBuffer(),
    ]);

    //const db = new sqlite3.oo1.DB();
    const db = new sqlite3.oo1.JsStorageDb('local');

    const rc = sqlite3.capi.sqlite3_deserialize(
        db.pointer,
        'main',
        sqlite3.wasm.allocFromTypedArray(dataBuffer),
        dataBuffer.byteLength,
        dataBuffer.byteLength,
        sqlite3.capi.SQLITE_DESERIALIZE_FREEONCLOSE,
    );
    db.checkRc(rc);

    return { sqlite3, db };
}

export function getTimeWindow(startStr, endStr) {
    const startDate = new Date(startStr);
    const endDate = new Date(endStr);
    const durationHours = (endDate - startDate) / (1000 * 60 * 60);

    if (durationHours > 24 * 20) {
        return '1d';
    } else if (durationHours > 24 * 8) {
        return '1h';
    } else {
        return '30m';
    }
}

export async function getConsumption(db, startStr, endStr, type = 'IMPORT') {
    const timeWindow = getTimeWindow(startStr, endStr);
    const timeColIdx = ["1d", "1h", "30m"].indexOf(timeWindow);

    const query = new URLSearchParams({
        "start": startStr,
        "end": endStr,
        "type": type,
        "window": timeWindow,
    });

    let response = await fetch("/api/consumption?" + query.toString());
    let data = await response.json();
    return { data, timeWindow };
}

export function getPriceDistribution(db, startStr, endStr) {
    const timeWindow = getTimeWindow(startStr, endStr);
    const sql = `
        SELECT
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
        ON c.interval_start = import.valid_from AND c.interval_start = export.valid_from
        JOIN products as pimport ON import.product_code = pimport.product_code AND import.tariff_code = pimport.tariff_code
        JOIN products as pexport ON export.product_code = pexport.product_code AND export.tariff_code = pexport.tariff_code
        WHERE c.interval_start >= $start AND c.interval_start < $end
            AND pimport.type = 'IMPORT'
            AND pexport.type = 'EXPORT'
    `;

    const data = [];
    db.exec({
        sql: sql,
        bind: {
            $start: startStr,
            $end: endStr,
        },
        callback: (row) => {
            data.push({
                import_rate: row[0],
                export_rate: row[1],
                consumption: row[2],
                generation: row[3],
                slots: row[4],
                cost: row[5],
                sale: row[6],
            });
        },
    });
    return { data, timeWindow };
}

export function getConsumptionByTimeOfDay(db, startStr, endStr) {
    // Query all consumption data within the window, grouped by 30-minute time of day
    const sql = `
        SELECT
            strftime('%H:%M', c.interval_start) as t_30m,
            import.value as import_rate,
            export.value as export_rate,
            SUM(c.consumption) as consumption,
            SUM(c.generation) as generation
        FROM consumption as c
        LEFT JOIN tariff_rates as import ON c.interval_start = import.valid_from
        LEFT JOIN tariff_rates as export ON c.interval_start = export.valid_from
        JOIN products as pimport ON import.product_code = pimport.product_code AND pimport.tariff_code = pimport.tariff_code
        JOIN products as pexport ON export.product_code = pexport.product_code AND pexport.tariff_code = pexport.tariff_code
        WHERE c.interval_start >= $start AND c.interval_start < $end
          AND pimport.type = 'IMPORT'
          AND pexport.type = 'EXPORT'
        GROUP BY t_30m, import_rate
        ORDER BY t_30m ASC, import_rate DESC
    `;

    const data = [];
    db.exec({
        sql: sql,
        bind: {
            $start: startStr,
            $end: endStr,
        },
        callback: (row) => {
            const timeStr = row[0];
            const [hours, minutes] = timeStr.split(':').map(Number);

            // Create a date object for today with the time of day
            const timestamp = new Date();
            timestamp.setHours(hours, minutes, 0, 0);

            // Create end timestamp (30 minutes later)
            const timestampEnd = new Date(timestamp);
            timestampEnd.setMinutes(timestampEnd.getMinutes() + 30);

            data.push({
                time_of_day: timeStr,
                timestamp: timestamp,
                timestampEnd: timestampEnd,
                import_rate: row[1],
                export_rate: row[2],
                consumption: row[3],
                generation:  row[4],
            });
        },
    });

    return { data, timeWindow: '30m' };
}

export function getStandingCharge(db, startStr, endStr, type = 'IMPORT') {
    /**
     * Returns a map of date key (YYYY-MM-DD) to standing charge in pence.
     * Queries tariff_rates table for standing charges active during the period.
     */
    const sql = `
        SELECT
            date(r.valid_from) as day,
            AVG(r.daily_standing_charge)
        FROM tariff_rates as r
        JOIN products as p ON r.product_code = p.product_code AND r.tariff_code = p.tariff_code
        WHERE r.valid_from >= $start AND r.valid_from < $end
          AND p.type = $type
        GROUP BY day
    `;

    const dateMap = new Map();
    db.exec({
        sql: sql,
        bind: {
            $start: startStr,
            $end: endStr,
            $type: type
        },
        callback: (row) => {
            dateMap.set(row[0], row[1]);
        },
    });

    return dateMap;
}

export function getSlotCountsByDay(db, startStr, endStr, type = 'IMPORT') {
    const sql = `
        SELECT
            date(c.interval_start) as day,
            COUNT(*) as slots
        FROM consumption as c
        LEFT JOIN tariff_rates as r ON c.interval_start = r.valid_from
        JOIN products as p ON r.product_code = p.product_code AND r.tariff_code = p.tariff_code
        WHERE c.interval_start >= $start AND c.interval_start < $end
          AND p.type = $type
        GROUP BY date(c.interval_start)
        ORDER BY day ASC
    `;

    const map = new Map();
    db.exec({
        sql: sql,
        bind: { $start: startStr, $end: endStr, $type: type },
        callback: (row) => {
            map.set(row[0], row[1]);
        }
    });

    return map;
}

export function getAgilePredictions(db, startStr, endStr, region) {
    const timeWindow = getTimeWindow(startStr, endStr);
    const timeColIdx = ["1d", "1h", "30m"].indexOf(timeWindow);

    const sql = `
        SELECT
            strftime('%Y-%m-%dT00:00:00Z', timestamp), -- daily
            strftime('%Y-%m-%dT%H:00:00Z', timestamp), -- hourly
            timestamp,                                 -- half-hourly

            AVG(prediction)
        FROM agile_predictions
        WHERE
            region = $region
        AND timestamp >= $start
        AND timestamp < $end
        GROUP BY
          CASE $window
          WHEN '1d' THEN date(timestamp)
          WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', timestamp)
          ELSE timestamp
        END
        ORDER BY timestamp ASC
    `;

    const data = [];
    db.exec({
        sql: sql,
        bind: { $start: startStr, $end: endStr, $region: region, $window: timeWindow },
        callback: (row) => {
            data.push({
                timestamp: new Date(row[timeColIdx]),
                prediction: row[3],
            });
        }
    });

    return data;
}

export function getDataLimits(db) {
    /**
     * Returns the earliest and latest timestamps from consumption data.
     * Returns null for both if no data exists.
     */
    const sql = `
        SELECT MIN(interval_start), MAX(interval_start)
        FROM consumption
    `;

    let earliestDate = null;
    let latestDate = null;

    db.exec({
        sql: sql,
        callback: (row) => {
            if (row[0]) earliestDate = new Date(row[0]);
            if (row[1]) latestDate = new Date(row[1]);
        }
    });

    return { earliestDate, latestDate };
}
