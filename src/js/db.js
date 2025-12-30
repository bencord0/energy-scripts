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
