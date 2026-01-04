import { default as sqlite3WasmInit } from '/js/sqlite-wasm-3510100/jswasm/sqlite3.mjs';

const DB_VERSION = '2026-01-04T22:19:31Z';

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

export function getConsumptionTimeSeries(db, startStr, endStr, type = 'IMPORT') {
    const startDate = new Date(startStr);
    const endDate = new Date(endStr);
    const durationHours = (endDate - startDate) / (1000 * 60 * 60);

    let sql = '';
    let grouping = '30m';

    if (durationHours > 24 * 30) {
        // More than a month: Group by Day
        sql = `
            SELECT
                date(r.valid_from) || 'T00:00:00Z',
                sum(c.consumption),
                avg(r.value),
                sum(c.consumption * r.value) as cost
            FROM tariff_rates as r
            JOIN products as p ON r.product_code = p.product_code AND r.tariff_code = p.tariff_code
            LEFT JOIN consumption as c ON r.valid_from = c.interval_start
            WHERE r.valid_from >= $start AND r.valid_from < $end
              AND p.type = $type
            GROUP BY date(r.valid_from)
            ORDER BY r.valid_from ASC
        `;
        grouping = '1d';
    } else if (durationHours > 24 * 7) {
        // More than a week: Group by Hour
        sql = `
            SELECT
                strftime('%Y-%m-%dT%H:00:00Z', r.valid_from),
                sum(c.consumption),
                avg(r.value),
                sum(c.consumption * r.value) as cost
            FROM tariff_rates as r
            JOIN products as p ON r.product_code = p.product_code AND r.tariff_code = p.tariff_code
            LEFT JOIN consumption as c ON r.valid_from = c.interval_start
            WHERE r.valid_from >= $start AND r.valid_from < $end
              AND p.type = $type
            GROUP BY strftime('%Y-%m-%dT%H', r.valid_from)
            ORDER BY r.valid_from ASC
        `;
        grouping = '1h';
    } else {
        // Default: 30 minute intervals
        sql = `
            SELECT
                r.valid_from,
                c.consumption,
                r.value,
                (c.consumption * r.value) as cost
            FROM tariff_rates as r
            JOIN products as p ON r.product_code = p.product_code AND r.tariff_code = p.tariff_code
            LEFT JOIN consumption as c ON r.valid_from = c.interval_start
            WHERE r.valid_from >= $start AND r.valid_from < $end
              AND p.type = $type
            ORDER BY r.valid_from ASC
        `;
        grouping = '30m';
    }

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
                timestamp: new Date(row[0]),
                consumption: row[1],
                rate: row[2],
                cost: row[3],
            });
        },
    });

    return { data, grouping };
}

export function getPriceDistribution(db, startStr, endStr, type = 'IMPORT') {
    const sql = `
        SELECT
            r.value,
            SUM(c.consumption),
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
                cost: row[2],
            });
        },
    });
    return data;
}
