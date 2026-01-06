import { default as sqlite3WasmInit } from '/js/sqlite-wasm-3510100/jswasm/sqlite3.mjs';

const DB_VERSION = '2026-01-06T19:45:15Z';

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

                if (dbVersion === DB_VERSION) {
                    const fetchDate = new Date(cachedResponse.headers.get('date'));
                    const now = new Date();
                    const oneHour = 3600 * 1000;

                    if (now - fetchDate < oneHour) {
                        console.log('Using cached database (version ' + dbVersion + ')');
                        return buffer;
                    }
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
    const sqlite3 = await getSqlite3();
    const dataBuffer = await getDataBuffer();

    const db = new sqlite3.oo1.DB();

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

    if (durationHours > 24 * 30) {
        return '1d';
    } else if (durationHours > 24 * 7) {
        return '1h';
    } else {
        return '30m';
    }
}

export function getConsumption(db, startStr, endStr, type = 'IMPORT') {
    const timeWindow = getTimeWindow(startStr, endStr);
    const timeColIdx = ["1d", "1h", "30m"].indexOf(timeWindow);

    const sql = `
        SELECT
            date(r.valid_from) || 'T00:00:00Z',
            strftime('%Y-%m-%dT%H:00:00Z', r.valid_from),
            r.valid_from,
            SUM(c.consumption),
            AVG(r.value),
            SUM(c.consumption * r.value)
        FROM tariff_rates as r
        JOIN products as p ON r.product_code = p.product_code AND r.tariff_code = p.tariff_code
        LEFT JOIN consumption as c ON r.valid_from = c.interval_start
        WHERE r.valid_from >= $start AND r.valid_from < $end
          AND p.type = $type
        GROUP BY
          CASE $window
            WHEN '1d' THEN date(r.valid_from)
            WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', r.valid_from)
            ELSE r.valid_from
          END
        ORDER BY r.valid_from ASC
    `;

    const data = [];

    db.exec({
        sql: sql,
        bind: {
            $start: startStr,
            $end: endStr,
            $type: type,
            $window: timeWindow
        },
        callback: (row) => {
            data.push({
                timestamp: new Date(row[timeColIdx]),
                consumption: row[3],
                rate: row[4],
                cost: row[5],
            });
        },
    });

    return { data, timeWindow };
}

export function getPriceDistribution(db, startStr, endStr, type = 'IMPORT') {
    const timeWindow = getTimeWindow(startStr, endStr);
    const sql = `
        SELECT
            r.value,
            SUM(c.consumption),
            COUNT(c.interval_start) as slots,
            (r.value * SUM(c.consumption)) as cost
        FROM consumption as c
        LEFT JOIN tariff_rates as r
        ON c.interval_start = r.valid_from
        JOIN products as p ON r.product_code = p.product_code AND r.tariff_code = p.tariff_code
        WHERE c.interval_start >= $start AND c.interval_start < $end
          AND p.type = $type
        GROUP BY r.value
        ORDER BY r.value ASC
    `;

    const data = [];
    db.exec({
        sql: sql,
        bind: {
            $start: startStr,
            $end: endStr,
            $type: type
        },
        callback: (row) => {
            data.push({
                rate: row[0],
                consumption: row[1],
                slots: row[2],
                cost: row[3],
            });
        },
    });
    return { data, timeWindow };
}

export function getTimingByTimeOfDay(db, startStr, endStr, type = 'IMPORT') {
    const timeWindow = getTimeWindow(startStr, endStr);

    const timeColIdx = ["1d", "1h", "30m"].indexOf(timeWindow);
    const intervalMinutes = {
        "1d": 1440,
        "1h": 60,
        "30m": 30,
    }[timeWindow];

    // Query all consumption data within the window
    // Calculate variants for different time windows in a single expression
    const sql = `
        SELECT
            '00:00' as t_1d,
            strftime('%H:00', c.interval_start) as t_1h,
            strftime('%H:%M', c.interval_start) as t_30m,
            r.value as rate,
            SUM(c.consumption) as consumption
        FROM consumption as c
        LEFT JOIN tariff_rates as r ON c.interval_start = r.valid_from
        JOIN products as p ON r.product_code = p.product_code AND r.tariff_code = p.tariff_code
        WHERE c.interval_start >= $start AND c.interval_start < $end
          AND p.type = $type
        GROUP BY t_30m, rate
        ORDER BY t_30m ASC, rate DESC
    `;

    const data = [];
    db.exec({
        sql: sql,
        bind: {
            $start: startStr,
            $end: endStr,
            $type: type
        },
        callback: (row) => {
            const timeStr = row[timeColIdx];
            const rate = row[3];
            const consumption = row[4];
            const [hours, minutes] = timeStr.split(':').map(Number);

            // Create a date object for today with the time of day
            const timestamp = new Date();
            timestamp.setHours(hours, minutes, 0, 0);

            // Create end timestamp
            const timestampEnd = new Date(timestamp);
            timestampEnd.setMinutes(timestampEnd.getMinutes() + intervalMinutes);

            data.push({
                time_of_day: timeStr,
                timestamp: timestamp,
                timestampEnd: timestampEnd,
                rate: rate,
                consumption: consumption,
            });
        },
    });

    return { data, timeWindow };
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
