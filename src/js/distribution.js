import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { default as sqlite3WasmInit } from '/js/sqlite-wasm-3510100/jswasm/sqlite3.mjs';

async function getDataBuffer() {
    const cacheName = 'octopus-data-v1';
    const url = '/data/power.sqlite3';

    // Speed up first page load if we have visited the site before
    if ('caches' in window) {
        const cache = await caches.open(cacheName);
        const cachedResponse = await cache.match(url);

        if (cachedResponse) {
            let lastModified = new Date(cachedResponse.headers.get('last-modified'));
            let cacheResetTime = new Date(new Date().setHours(16, 0, 0, 0));
            let now = new Date();

            if (lastModified < cacheResetTime && now < cacheResetTime) {
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

const [sqlite3, dataBuffer] = await Promise.all([
    sqlite3WasmInit(),
    getDataBuffer()
]);
window.sqlite3 = sqlite3; // for debugging

// open an empty database
const db = new sqlite3.oo1.DB();
window.db = db; // for debugging

const rc = sqlite3.capi.sqlite3_deserialize(
    db.pointer,
    'main', // primary database schema to overwrite
    sqlite3.wasm.allocFromTypedArray(dataBuffer),
    dataBuffer.byteLength,
    dataBuffer.byteLength,
    sqlite3.capi.SQLITE_DESERIALIZE_FREEONCLOSE,
);
// throws SQLite3Error
db.checkRc(rc);

function render() {
    // https://observablehq.com/blog/reshaping-data-plot-d3
    // https://r4ds.had.co.nz/tidy-data.html
    // Expect data in a "tidy" format.
    const data = [];

    let sql = `
    SELECT
        r.value,
        SUM(c.consumption),
        (r.value * SUM(consumption)) as cost
    FROM consumption as c
    LEFT JOIN tariff_rates as r
    ON c.interval_start = r.valid_from
    WHERE c.interval_start > '2025-12-27'
    GROUP BY r.value
    ORDER BY r.value ASC`;

    db.exec({
        sql: sql,
        callback: (row) => {
            data.push({
                rate: row[0],
                consumption: row[1],
                cost: row[2],
            });
        },
    });

    function formatCost(cost) {
        if (cost > 100) {
            const pounds = cost / 100;
            return `£${pounds.toFixed(2)}`;
        } else {
            return `${cost.toFixed(2)}p`;
        }
    }

    const maxX = d3.max(data, d => d.rate);
    const ceilX = Math.ceil(maxX / 10) * 10;

    const plot = Plot.plot({
        height: window.innerHeight - 40,
        width: window.innerWidth - 40,
        x: {
            label: "rate (p/kWh)",
            ticks: ceilX / 5,
            domain: [0, ceilX],
        },
        y: {
            label: "usage (kWh)",
            grid: true,
        },
        marks: [
            // Cost
            Plot.rectY(data, Plot.binX({
                y: "sum",
                title: (bin) => {
                    const rate = d3.min(bin, d => d.rate);
                    const usage = d3.sum(bin, d => d.consumption);
                    const cost = d3.sum(bin, d => d.cost);
                    return `Rate: ${rate.toFixed(2)}p/kWh\nUsage: ${usage.toFixed(3)} kWh\nCost: ${formatCost(cost)}`;
                }
            }, {
                x: "rate",
                y: "consumption",
                title: (d) => d,
                fill: "steelblue",
                interval: 0.5,
                tip: true
            })),
            Plot.axisY({ anchor: "left", label: "Used Energy (kWh)" }),
        ],
    });

    const chart = document.getElementById("chart");
    chart.style.cursor = 'crosshair';

    // Remove spinner if present
    const spinner = chart.querySelector(".spinner");
    if (spinner) spinner.remove();

    // Render plot into #plot div
    const plotDiv = document.getElementById("plot");
    if (plotDiv) {
        plotDiv.replaceChildren(plot);
    }
}

// Initial render
render();

let pendingUpdate = false;
window.addEventListener('resize', () => {
    if (!pendingUpdate) {
        pendingUpdate = true;
        requestAnimationFrame(() => {
            render();
            pendingUpdate = false;
        });
    }
});
