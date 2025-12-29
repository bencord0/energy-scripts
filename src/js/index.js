import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { default as sqlite3WasmInit } from '/js/sqlite-wasm-3510100/jswasm/sqlite3.mjs';

// https://observablehq.com/blog/reshaping-data-plot-d3
// https://r4ds.had.co.nz/tidy-data.html
// Expect data in a "tidy" format.
var data = [];

async function getDatabaseBuffer() {
    const cacheName = 'octopus-data-v1';
    const url = '/data/power.sqlite3';

    // Speed up first page load if we have visited the site before
    // TODO: Set a cache key based on etag.
    if ('caches' in window) {
        const cache = await caches.open(cacheName);
        const cachedResponse = await cache.match(url);
        if (cachedResponse) {
            console.log('Using cached database');
            return await cachedResponse.arrayBuffer();
        }
        console.log('Fetching database and caching...');
        const response = await fetch(url);
        await cache.put(url, response.clone());
        return await response.arrayBuffer();
    }

    return await fetch(url).then(res => res.arrayBuffer());
}

const [sqlite3, arrayBuffer] = await Promise.all([
    sqlite3WasmInit(),
    getDatabaseBuffer()
]);
window.sqlite3 = sqlite3; // for debugging

// open an empty database
const db = new sqlite3.oo1.DB();
window.db = db; // for debugging

const rc = sqlite3.capi.sqlite3_deserialize(
    db.pointer,
    'main', // primary database schema to overwrite
    sqlite3.wasm.allocFromTypedArray(arrayBuffer),
    arrayBuffer.byteLength,
    arrayBuffer.byteLength,
    sqlite3.capi.SQLITE_DESERIALIZE_FREEONCLOSE,
);
db.checkRc(rc);

const urlParams = new URLSearchParams(window.location.search);

let startDate, endDate;

if (urlParams.has('start') && urlParams.has('end')) {
    startDate = new Date(urlParams.get('start'));
    endDate = new Date(urlParams.get('end'));
} else {
    // Fallback if no data or error: 2 days ago to yesterday
    const msPerDay = 864e5;
    const [dayBefore, yesterday] = [2, 1].map(d => new Date(new Date().setHours(0, 0, 0, 0) - d * msPerDay));
    startDate = dayBefore;
    endDate = yesterday;
}

let urlDebounceTimeout;
function updateUrlDebounced(startStr, endStr) {
    clearTimeout(urlDebounceTimeout);
    urlDebounceTimeout = setTimeout(() => {
        const url = new URL(window.location);
        url.searchParams.set('start', startStr);
        url.searchParams.set('end', endStr);
        window.history.replaceState({}, '', url);
    }, 500);
}

function render() {
    const data = [];
    const startStr = startDate.toISOString().slice(0, 16);
    const endStr = endDate.toISOString().slice(0, 16);

    const durationHours = (endDate - startDate) / (1000 * 60 * 60);

    let sql = '';
    let interval = d3.timeMinute.every(30);

    if (durationHours > 24 * 30) {
        // More than a month: Group by Day
        sql = `
            SELECT
                date(c.interval_start) || 'T00:00:00Z',
                sum(c.consumption),
                avg(r.value),
                sum(c.consumption * r.value)
            FROM consumption as c
            LEFT JOIN tariff_rates as r ON c.interval_start = r.valid_from
            WHERE c.interval_start >= $start AND c.interval_start < $end
            GROUP BY date(c.interval_start)
            ORDER BY c.interval_start ASC
        `;
        interval = d3.timeDay;
    } else if (durationHours > 24 * 7) {
        // More than a week: Group by Hour
        sql = `
            SELECT
                strftime('%Y-%m-%dT%H:00:00Z', c.interval_start),
                sum(c.consumption),
                avg(r.value),
                sum(c.consumption * r.value)
            FROM consumption as c
            LEFT JOIN tariff_rates as r ON c.interval_start = r.valid_from
            WHERE c.interval_start >= $start AND c.interval_start < $end
            GROUP BY strftime('%Y-%m-%dT%H', c.interval_start)
            ORDER BY c.interval_start ASC
        `;
        interval = d3.timeHour;
    } else {
        // Default: 30 minute intervals
        sql = `
            SELECT
                c.interval_start,
                c.consumption,
                r.value,
                (c.consumption * r.value) as cost
            FROM consumption as c
            LEFT JOIN tariff_rates as r ON c.interval_start = r.valid_from
            WHERE c.interval_start >= $start AND c.interval_start < $end
            ORDER BY c.interval_start ASC
        `;
    }

    db.exec({
        sql: sql,
        bind: {
            $start: startStr,
            $end: endStr,
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

    const maxKWh = d3.max(data, d => d.consumption) || 1;
    const maxP = d3.max(data, d => Math.max(d.rate, d.cost)) || 40;
    const scaleFactor = maxP / maxKWh;

    function formatTick(d) {
        const isMidnight = d.getHours() === 0 && d.getMinutes() === 0;
        if (durationHours <= 24) return d3.timeFormat("%H:%M")(d);
        if (isMidnight) {
            if (d.getMonth() === 0 && d.getDate() === 1) return d3.timeFormat("%Y")(d);
            return d3.timeFormat("%b %d")(d);
        }
        return durationHours > 48 ? "" : d3.timeFormat("%H:%M")(d);
    }

    const plot = Plot.plot({
        width: window.innerWidth - 40,
        x: {
            type: "time",
            label: "timestamp",
            tickFormat: formatTick,
            ticks: 12,
            domain: [startDate, endDate],
        },
        y: { grid: true },
        color: {
            type: "threshold",
            domain: [7, 14, 27, 31],
            range: [
                "#0077be", // < 7: Less than Intelligent Octopus Off-Peak
                "#52be80", // 7-14: Less than Economy 7 Night Rate
                "#f1c40f", // 14-27: Less than the Flexible Rate (equivalent to the Ofgem Price Cap)
                "#e67e22", // 27-31: Less than the IOG Day Rate
                "#e74c3c"  // > 31: More than the Cosy Peak Rate
            ]
        },
        marks: [
            // Cost
            Plot.rectY(data, {
                x: 'timestamp',
                y: d => d.cost / scaleFactor,
                interval: interval,
                fill: 'rate',
            }),
            // Rate
            Plot.rectY(data, {
                x: 'timestamp',
                y: d => d.rate / scaleFactor,
                interval: interval,
                fill: "#ccc",
                fillOpacity: 0.2,
                mixBlendMode: "multiply",
            }),
            // Consumption
            Plot.lineY(data, {
                x: 'timestamp',
                y: 'consumption',
                stroke: "rgba(0, 127, 200, 0.8)",
                strokeWidth: 2,
            }),
            Plot.axisY({ anchor: "left", label: "Used Energy (kWh)" }),
            Plot.axisY({
                anchor: "right",
                label: "rate (p/kWh), cost (p)",
                tickFormat: y => (y * scaleFactor).toFixed(0),
            }),
            Plot.tip(data, Plot.pointerX({
                x: "timestamp",
                y: d => d3.max([d.cost / scaleFactor, d.rate / scaleFactor, d.consumption]),
                title: d => [
                    `Time: ${d3.timeFormat("%Y-%m-%d %H:%M")(d.timestamp)}`,
                    `Usage: ${d.consumption.toFixed(3)} kWh`,
                    `Rate: ${d.rate.toFixed(2)} p/kWh`,
                    `Cost: ${d.cost.toFixed(2)} p`
                ].join("\n")
            })),
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
    } else {
        // Fallback (though structure should be in index.html)
        chart.appendChild(plot);
    }

    // Calculate total cost
    const totalCost = data.reduce((sum, d) => sum + (d.cost || 0), 0);

    // Calculate hours covered (based on actual data points)
    const hoursFromData = data.length > 0
        ? (data[data.length - 1].timestamp - data[0].timestamp) / (1000 * 60 * 60)
        : 0;

    // Calculate average hourly cost
    const avgHourlyCost = hoursFromData > 0 ? totalCost / hoursFromData : 0;

    // Calculate average rate (independent of consumption)
    const avgRate = data.length > 0
        ? data.reduce((sum, d) => sum + (d.rate || 0), 0) / data.length
        : 0;

    // Calculate total consumption
    const totalConsumption = data.reduce((sum, d) => sum + (d.consumption || 0), 0);

    // Calculate interval duration in hours (for power calculation)
    // The consumption values are energy (kWh) over the interval period
    let intervalHours = 0.5; // Default: 30 minutes
    if (durationHours > 24 * 30) {
        intervalHours = 24; // Daily aggregation
    } else if (durationHours > 24 * 7) {
        intervalHours = 1; // Hourly aggregation
    }

    // Calculate average and max power (kW)
    const avgPower = data.length > 0
        ? data.reduce((sum, d) => sum + (d.consumption || 0), 0) / data.length / intervalHours
        : 0;
    const maxPower = data.length > 0
        ? Math.max(...data.map(d => (d.consumption || 0) / intervalHours))
        : 0;

    // Format cost display: £ for values over 100p, p for values 100p or less
    function formatCost(cost) {
        if (cost > 100) {
            const pounds = cost / 100;
            return `£${pounds.toFixed(2)}`;
        } else {
            return `${cost.toFixed(2)}`;
        }
    }

    // Update Cost Summary Values in DOM
    const periodElem = document.getElementById('val-period');
    if (periodElem) periodElem.textContent = hoursFromData.toFixed(1);

    const powerTotalElem = document.getElementById('val-power-total');
    if (powerTotalElem) powerTotalElem.textContent = totalConsumption.toFixed(2);

    const powerAvgElem = document.getElementById('val-power-avg');
    if (powerAvgElem) powerAvgElem.textContent = avgPower.toFixed(3);

    const powerMaxElem = document.getElementById('val-power-max');
    if (powerMaxElem) powerMaxElem.textContent = maxPower.toFixed(3);

    // For elements with HTML content (like the £/p span)
    const costTotalElem = document.getElementById('val-cost-total');
    if (costTotalElem) costTotalElem.innerHTML = formatCost(totalCost);

    const costHourlyElem = document.getElementById('val-cost-hourly');
    if (costHourlyElem) costHourlyElem.innerHTML = formatCost(avgHourlyCost);

    const costRateElem = document.getElementById('val-cost-rate');
    if (costRateElem) costRateElem.textContent = avgRate.toFixed(2);

    // Update URL without refreshing (Debounced)
    updateUrlDebounced(startStr, endStr);
}

// Initial render
render();

let pendingUpdate = false;
window.addEventListener('wheel', (e) => {
    e.preventDefault();

    const duration = endDate.getTime() - startDate.getTime();
    const rect = document.getElementById('chart').getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseRatio = Math.max(0, Math.min(1, mouseX / rect.width));
    const focusTime = startDate.getTime() + duration * mouseRatio;

    // Zoom (Vertical scroll)
    if (e.deltaY !== 0) {
        const factor = Math.pow(1.1, e.deltaY / 100);
        const newDuration = Math.max(1000 * 60 * 30, Math.min(duration * factor, 1000 * 60 * 60 * 24 * 365));
        startDate = new Date(focusTime - newDuration * mouseRatio);
        endDate = new Date(focusTime + newDuration * (1 - mouseRatio));
    }

    // Scroll (Horizontal scroll)
    if (e.deltaX !== 0) {
        const shift = (e.deltaX / rect.width) * duration;
        startDate = new Date(startDate.getTime() + shift);
        endDate = new Date(endDate.getTime() + shift);
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
