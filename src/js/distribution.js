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

const urlParams = new URLSearchParams(window.location.search);

const millisecondsPerDay = 864e5;
let today = new Date().setHours(0, 0, 0, 0);
let yesterday = new Date(today - millisecondsPerDay);
let dayBefore = new Date(yesterday - millisecondsPerDay);
let startDate = new Date(urlParams.get('start') || dayBefore);
let endDate = new Date(urlParams.get('end') || yesterday);

let urlDebouncer;
function updateUrl(startStr, endStr) {
    clearTimeout(urlDebouncer);
    urlDebouncer = setTimeout(() => {
        const url = new URL(window.location);
        url.searchParams.set('start', startStr);
        url.searchParams.set('end', endStr);
        window.history.replaceState({}, '', url);
    }, 500);
}

function render() {
    // https://observablehq.com/blog/reshaping-data-plot-d3
    // https://r4ds.had.co.nz/tidy-data.html
    // Expect data in a "tidy" format.
    const data = [];

    const startStr = startDate.toISOString().slice(0, 16);
    const endStr = endDate.toISOString().slice(0, 16);

    let sql = `
    SELECT
        r.value,
        SUM(c.consumption),
        (r.value * SUM(consumption)) as cost
    FROM consumption as c
    LEFT JOIN tariff_rates as r
    ON c.interval_start = r.valid_from
    WHERE c.interval_start > $start AND c.interval_end < $end
    GROUP BY r.value
    ORDER BY r.value ASC`;

    db.exec({
        sql: sql,
        bind: {
            $start: startStr,
            $end: endStr,
        },
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

    const maxX = d3.max(data, d => d.rate) || 0;
    const ceilX = Math.ceil(maxX / 10) * 10;
    const interval = 0.5;

    // 1. Pre-calculate bins
    const thresholds = d3.range(0, ceilX + interval, interval);
    const binFn = d3.bin()
        .value(d => d.rate)
        .domain([0, ceilX])
        .thresholds(thresholds);

    const inputs = binFn(data).map(bin => {
        // bin is an array of data points, with x0 and x1
        const consumption = d3.sum(bin, d => d.consumption);
        const cost = d3.sum(bin, d => d.cost);
        return {
            rate_start: bin.x0,
            rate_end: bin.x1,
            rate_mid: (bin.x0 + bin.x1) / 2,
            consumption: consumption,
            cost: cost
        };
    }).filter(d => d.consumption > 0);

    const maxY = d3.max(inputs, d => d.cost) || 0;

    // 2. Generate Iso-Usage Lines
    // Cost (y) = Usage (m) * Rate (x)
    // These are straight lines y = mx
    // Find roughly the max usage in a bin to determine steps
    const maxBarUsage = d3.max(inputs, d => d.consumption) || 0;

    // Determine order of magnitude for steps
    const stepUsage = Math.pow(10, Math.floor(Math.log10(maxBarUsage || 1)));
    const isoUsage = [];

    // Generate 1x, 2x, 5x, 10x steps
    [0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50].forEach(m => {
        const u = stepUsage * m;
        // Filter out lines that are too small or too large relative to data
        if (u > maxBarUsage * 1.5 && u > (maxY / (ceilX || 1))) return;
        if (u < maxBarUsage / 20) return;

        // Line: y = u * x
        // x2 is either the max of chart or where it hits top of chart
        // y2 = u * ceilX
        const x2 = ceilX;
        const y2 = u * x2;

        const line = [{ x: 0, y: 0, u: u }];
        line.push({ x: x2, y: y2, u: u });

        isoUsage.push(line);
    });

    const plot = Plot.plot({
        height: window.innerHeight - 40,
        width: window.innerWidth - 40,
        x: {
            label: "rate (p/kWh)",
            domain: [0, ceilX],
        },
        y: {
            label: "Cost",
            grid: true,
            domain: [0, maxY * 1.1], // give some headroom
        },
        marks: [
            // Iso-Usage Lines (Radiating lines)
            isoUsage.map(curve => Plot.line(curve, {
                x: "x",
                y: "y",
                stroke: "pink",
                strokeDasharray: "4,4",
                strokeWidth: 1
            })),
            isoUsage.map(curve => {
                const last = curve[1];
                return Plot.text([last], {
                    x: "x",
                    y: "y",
                    text: d => `${d.u.toFixed(1)} kWh`,
                    fill: "pink",
                    dx: 5,
                    dy: -5,
                    textAnchor: "start"
                });
            }),

            // Cost Bars
            Plot.rectY(inputs, {
                x1: "rate_start",
                x2: "rate_end",
                y: "cost",
                fill: "steelblue",
                tip: true,
                title: d => `Rate: ${d.rate_start} - ${d.rate_end} p/kWh\nUsage: ${d.consumption.toFixed(3)} kWh\nCost: ${formatCost(d.cost)}`
            }),
            Plot.axisY({
                anchor: "left",
                label: "Cost",
                tickFormat: (d) => `£${(d / 100).toFixed(2)}`
            }),
        ],
        marginRight: 80, // space for iso labels
        marginLeft: 60,
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

    // Update URL without refreshing
    updateUrl(startStr, endStr);
}

// Initial render
render();

let pendingUpdate = false;
window.addEventListener('wheel', (e) => {
    e.preventDefault();

    if (e.deltaY !== 0) {
        startDate.setHours(startDate.getHours() + e.deltaY);
        if (startDate > dayBefore) {
            startDate.setTime(dayBefore.getTime());
        }

        // limit of our data
        let earliestDay = new Date("2025-12-01");
        if (startDate < earliestDay) {
            startDate.setTime(earliestDay.getTime());
        }
    }

    if (!pendingUpdate) {
        pendingUpdate = true;
        requestAnimationFrame(() => {
            render();
            pendingUpdate = false;
       });
   }
}, { passive: false });

window.addEventListener('resize', () => {
    if (!pendingUpdate) {
        pendingUpdate = true;
        requestAnimationFrame(() => {
            render();
            pendingUpdate = false;
        });
    }
});
