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

export async function getPriceDistribution(db, startStr, endStr) {
    const query = new URLSearchParams({
        "start": startStr,
        "end": endStr,
    });

    let response = await fetch("/api/price-distribution?" + query.toString());
    let data = await response.json();

    const timeWindow = getTimeWindow(startStr, endStr);
    return { data, timeWindow };
}

export async function getConsumptionByTimeOfDay(db, startStr, endStr) {
    const query = new URLSearchParams({
        "start": startStr,
        "end": endStr,
    });

    let response = await fetch("/api/consumption-by-time?" + query.toString());
    let data = await response.json();

    const timeWindow = getTimeWindow(startStr, endStr);
    return { data, timeWindow };
}

export async function getStandingCharge(db, startStr, endStr, type = 'IMPORT') {
    const query = new URLSearchParams({
        "start": startStr,
        "end": endStr,
        "type": type,
    });

    let response = await fetch("/api/standing-charge?" + query.toString());
    let data = await response.json();

    return data;
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
