export async function getDataBuffer() {
    const cacheName = 'octopus-data-v1';
    const url = '/data/power.sqlite3';

    // Speed up first page load if we have visited the site before
    if ('caches' in window) {
        const cache = await caches.open(cacheName);
        const cachedResponse = await cache.match(url);

        if (cachedResponse) {
            const fetchDate = new Date(cachedResponse.headers.get('date'));
            const now = new Date();
            const oneHour = 3600 * 1000;

            if (now - fetchDate < oneHour) {
                console.log('Using cached database');
                return await cachedResponse.arrayBuffer();
            }
        }
        console.log('Fetching database and caching...');
        const response = await fetch(url);
        await cache.put(url, response.clone());
        return await response.arrayBuffer();
    }

    return await fetch(url).then(res => res.arrayBuffer());
}

import { default as sqlite3WasmInit } from '/js/sqlite-wasm-3510100/jswasm/sqlite3.mjs';

export async function initDatabase() {
    const [sqlite3, dataBuffer] = await Promise.all([
        sqlite3WasmInit(),
        getDataBuffer()
    ]);

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
            LEFT JOIN consumption as c ON r.valid_from = c.interval_start
            WHERE r.valid_from >= $start AND r.valid_from < $end
              AND r.type = $type
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
            LEFT JOIN consumption as c ON r.valid_from = c.interval_start
            WHERE r.valid_from >= $start AND r.valid_from < $end
              AND r.type = $type
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
            LEFT JOIN consumption as c ON r.valid_from = c.interval_start
            WHERE r.valid_from >= $start AND r.valid_from < $end
              AND r.type = $type
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
        WHERE c.interval_start >= $start AND c.interval_start < $end
          AND r.type = $type
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
