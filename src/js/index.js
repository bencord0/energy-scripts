import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { initDatabase } from '/js/db.js';
import { priceColors } from '/js/colors.js';

const { sqlite3, db } = await initDatabase();
window.sqlite3 = sqlite3; // for debugging
window.db = db; // for debugging

const urlParams = new URLSearchParams(window.location.search);

const millisecondsPerDay = 864e5;
let today = new Date().setHours(0, 0, 0, 0);
let tomorrow = new Date(today + millisecondsPerDay);
let yesterday = new Date(today - millisecondsPerDay);
let dayBefore = new Date(yesterday - millisecondsPerDay);
let startDate = new Date(urlParams.get('start') || yesterday);
let endDate = new Date(urlParams.get('end') || tomorrow);

let urlDebouncer;
function updateUrl(startStr, endStr) {
    // https://developer.mozilla.org/en-US/docs/Web/API/Window/setTimeout
    // https://developer.mozilla.org/en-US/docs/Web/API/Window/clearTimeout
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

    const durationHours = (endDate - startDate) / (1000 * 60 * 60);

    let sql = '';
    let interval = d3.timeMinute.every(30);

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
            GROUP BY date(r.valid_from)
            ORDER BY r.valid_from ASC
        `;
        interval = d3.timeDay;
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
            GROUP BY strftime('%Y-%m-%dT%H', r.valid_from)
            ORDER BY r.valid_from ASC
        `;
        interval = d3.timeHour;
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
            ORDER BY r.valid_from ASC
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
        height: window.innerHeight - 40,
        width: window.innerWidth - 40,
        x: {
            type: "time",
            label: "timestamp",
            tickFormat: formatTick,
            ticks: 12,
            domain: [startDate, endDate],
        },
        y: { grid: true },
        color: priceColors,
        marks: [
            // Cost
            Plot.rectY(data, {
                x: 'timestamp',
                y: d => d.cost / scaleFactor,
                interval: interval,
                fill: 'rate',
            }),
            // Price
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
                label: "price (p/kWh), cost (p)",
                tickFormat: y => (y * scaleFactor).toFixed(0),
            }),
            // Current Time Marker
            Plot.ruleX([new Date()], {
                stroke: "red",
                strokeWidth: 2,
                strokeDasharray: "4,4"
            }),
            Plot.tip(data, Plot.pointerX({
                x: "timestamp",
                y: d => d3.max([d.cost / scaleFactor, d.rate / scaleFactor, d.consumption]),
                title: d => [
                    `Time: ${d3.timeFormat("%Y-%m-%d %H:%M")(d.timestamp)}`,
                    `Usage: ${(d.consumption || 0).toFixed(3)} kWh`,
                    `Agile Price: ${(d.rate || 0).toFixed(2)} p/kWh`,
                    `Cost: ${(d.cost || 0).toFixed(2)} p`
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
    }

    // Calculate total cost
    const totalCost = data.reduce((sum, d) => sum + (d.cost || 0), 0);

    // Calculate hours covered (based on actual data points)
    const period = data.length > 0
        ? 0.5 + (data[data.length - 1].timestamp - data[0].timestamp) / (1000 * 60 * 60)
        : 0;

    // Calculate average hourly cost
    const avgHourlyCost = period > 0 ? totalCost / period : 0;

    // Calculate average price (independent of consumption)
    // This will vary depending on the time-of-use tariff, e.g. Octopus Agile.
    // For Fixed and Flexible tariffs, this is (mostly) constant.
    const avgPrice = data.length > 0
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

    // Format cost display: £ for values over 100p
    function formatCost(cost) {
        if (cost > 100) {
            const pounds = cost / 100;
            return `£${pounds.toFixed(2)}`;
        } else {
            return `${cost.toFixed(2)}p`;
        }
    }

    // Update Cost Summary Values in DOM
    const periodElem = document.getElementById('val-period');
    if (periodElem) periodElem.textContent = period.toFixed(1);

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

    const costPriceElem = document.getElementById('val-cost-price');
    if (costPriceElem) costPriceElem.textContent = avgPrice.toFixed(2);

    // Update URL without refreshing (Debounced)
    updateUrl(startStr, endStr);
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

// Touch handling state
let lastTouchX = null;
let lastTouchDist = null;

const chartElement = document.getElementById('chart');

chartElement.addEventListener('touchstart', (e) => {
    if (e.target.closest('#cost-summary')) return;

    if (e.touches.length === 1) {
        lastTouchX = e.touches[0].clientX;
    } else if (e.touches.length === 2) {
        const t1 = e.touches[0];
        const t2 = e.touches[1];
        lastTouchDist = Math.hypot(t2.clientX - t1.clientX, t2.clientY - t1.clientY);
        lastTouchX = (t1.clientX + t2.clientX) / 2;
    }
}, { passive: false });

chartElement.addEventListener('touchmove', (e) => {
    if (e.target.closest('#cost-summary')) return;
    if (e.cancelable) e.preventDefault();

    const rect = chartElement.getBoundingClientRect();
    const duration = endDate.getTime() - startDate.getTime();

    if (e.touches.length === 1 && lastTouchX !== null) {
        // Pan
        const currentX = e.touches[0].clientX;
        const deltaX = lastTouchX - currentX; // Drag left = move forward in time (view moves right)

        // Sensitivity factor could be adjusted. Currently 1 pixel = 1 pixel of time-width
        const shift = (deltaX / rect.width) * duration;
        startDate = new Date(startDate.getTime() + shift);
        endDate = new Date(endDate.getTime() + shift);

        lastTouchX = currentX;

    } else if (e.touches.length === 2 && lastTouchDist !== null) {
        // Pinch Zoom
        const t1 = e.touches[0];
        const t2 = e.touches[1];
        const currentDist = Math.hypot(t2.clientX - t1.clientX, t2.clientY - t1.clientY);
        const currentCenter = (t1.clientX + t2.clientX) / 2;

        // Calculate zoom factor
        // distance increased = zoom in (show smaller time range) -> factor < 1
        // distance decreased = zoom out (show larger time range) -> factor > 1
        // This is opposite to scroll wheel often, let's derive it:
        // desired new duration = old_duration * (old_dist / new_dist)

        const factor = lastTouchDist / currentDist;

        // Apply limits
        const newDuration = Math.max(1000 * 60 * 30, Math.min(duration * factor, 1000 * 60 * 60 * 24 * 365));

        // Calculate focus point relative to chart width
        const mouseX = currentCenter - rect.left;
        const mouseRatio = Math.max(0, Math.min(1, mouseX / rect.width));
        const focusTime = startDate.getTime() + duration * mouseRatio;

        startDate = new Date(focusTime - newDuration * mouseRatio);
        endDate = new Date(focusTime + newDuration * (1 - mouseRatio));

        lastTouchDist = currentDist;
        lastTouchX = currentCenter; // Update center for potential smooth transition to pan
    }

    if (!pendingUpdate) {
        pendingUpdate = true;
        requestAnimationFrame(() => {
            render();
            pendingUpdate = false;
        });
    }
}, { passive: false });

chartElement.addEventListener('touchend', (e) => {
    if (e.touches.length < 2) {
        lastTouchDist = null;
    }
    if (e.touches.length === 0) {
        lastTouchX = null;
    } else if (e.touches.length === 1) {
        // Reset single touch anchor effectively to avoid jumps
        lastTouchX = e.touches[0].clientX;
    }
}, { passive: false });

// Update every minute
setInterval(() => {
    if (!pendingUpdate) {
        pendingUpdate = true;
        requestAnimationFrame(() => {
            render();
            pendingUpdate = false;
        });
    }
}, 60 * 1000);
