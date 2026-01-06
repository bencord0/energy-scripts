import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { initDatabase, getConsumption, getStandingCharge } from '/js/db.js';
import { priceColors, addStripes } from '/js/colors.js';
import { formatCost, getTimeWindowInfo } from '/js/utils.js';

const { sqlite3, db } = await initDatabase();
window.sqlite3 = sqlite3; // for debugging
window.db = db; // for debugging

const urlParams = new URLSearchParams(window.location.search);

const millisecondsPerDay = 864e5;
const today = new Date().setHours(0, 0, 0, 0);
const tomorrow = new Date(today + millisecondsPerDay);
const yesterday = new Date(today - millisecondsPerDay);
const dayBefore = new Date(yesterday - millisecondsPerDay);
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
    const startStr = startDate.toISOString().slice(0, 16);
    const endStr = endDate.toISOString().slice(0, 16);

    const durationHours = (endDate - startDate) / (1000 * 60 * 60);

    const { data, timeWindow } = getConsumption(db, startStr, endStr, 'IMPORT');
    const standingChargeMap = getStandingCharge(db, startStr, endStr, 'IMPORT');

    const { slotsPerDay, intervalHours } = getTimeWindowInfo(timeWindow);
    const interval = {
        "1d": d3.timeDay,
        "1h": d3.timeHour,
        "30m": d3.timeMinute.every(30),
    }[timeWindow];

    function dayKeyUTC(date) {
        return `${date.getUTCFullYear()}-${String(date.getUTCMonth() + 1).padStart(2, '0')}-${String(date.getUTCDate()).padStart(2, '0')}`;
    }

    function standingChargeForDate(date) {
        const key = dayKeyUTC(date);
        return standingChargeMap.get(key) || 0;
    }

    const maxKWh = d3.max(data, d => d.consumption) || 1;

    // Compute maxP considering standing charge
    const maxP = d3.max(data, d => {
        const standingChargeFraction = standingChargeForDate(d.timestamp) / slotsPerDay;
        return Math.max(d.rate || 0, (d.cost || 0) + standingChargeFraction);
    }) || 40;
    const scaleFactor = maxP / maxKWh;

    // Precompute standing charge and usage stacked above the standing charge bar
    // Only include rows for slots where we have consumption data
    const slotsWithConsumption = data.filter(d => d.consumption !== null && d.consumption !== undefined);

    const standingChargeRows = slotsWithConsumption.map(d => {
        const standingChargePence = standingChargeForDate(d.timestamp) / slotsPerDay;
        return {
            timestamp: d.timestamp,
            rate: d.rate,
            y2: (standingChargePence) / scaleFactor,
            hasUsage: (d.consumption || 0) > 0,
        };
    });

    const usageRows = slotsWithConsumption.map(d => {
        const standingChargePence = standingChargeForDate(d.timestamp) / slotsPerDay;
        const standingChargeScaled = standingChargePence / scaleFactor;
        const usageScaled = (d.cost || 0) / scaleFactor;
        return {
            timestamp: d.timestamp,
            rate: d.rate,
            y1: standingChargeScaled,
            y2: standingChargeScaled + usageScaled,
        };
    });

    function formatTick(d) {
        const isMidnight = d.getHours() === 0 && d.getMinutes() === 0;
        if (isMidnight) {
            if (d.getMonth() === 0 && d.getDate() === 1) return d3.timeFormat("%Y")(d);
            return d3.timeFormat("%b %d")(d);
        }
        if (durationHours <= 24) return d3.timeFormat("%H:%M")(d);
        if (durationHours > 48) return "";
        return d3.timeFormat("%H:%M")(d);
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
        y: { grid: true, zero: true },
        color: priceColors,
        marks: [
            // Price
            Plot.rectY(data, {
                x: 'timestamp',
                y: d => d.rate / scaleFactor,
                interval: interval,
                fill: "#ccc",
                fillOpacity: 0.2,
                mixBlendMode: "multiply",
            }),
            Plot.rectY(standingChargeRows, {
                x: 'timestamp',
                y: 'y2',
                interval: interval,
                fill: d => d.hasUsage ? d.rate : '#e0e0e0',
                fillOpacity: d => d.hasUsage ? 0.22 : 0.6,
            }),
            Plot.rectY(standingChargeRows, {
                x: 'timestamp',
                y: 'y2',
                interval: interval,
                fill: 'url(#stripes)',
                fillOpacity: 1,
                stroke: 'none',
                mixBlendMode: 'multiply',
            }),
            // Usage stacked above standing charge using y1/y2
            Plot.rectY(usageRows, {
                x: 'timestamp',
                y1: 'y1',
                y2: 'y2',
                interval: interval,
                fill: 'rate',
            }),
            // Consumption
            Plot.lineY(data, {
                x: 'timestamp',
                y: 'consumption',
                stroke: "rgba(0, 127, 200, 0.8)",
                strokeWidth: 2,
            }),
            // Baseline at zero to anchor bars
            Plot.ruleY([0]),
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
                y: function(d) {
                    let standingChargeFraction = 0;
                    if (d.consumption !== null && d.consumption !== undefined) {
                        standingChargeFraction = standingChargeForDate(d.timestamp) / slotsPerDay;
                    }
                    const costValue = (d.cost || 0) + standingChargeFraction;
                    const rateValue = d.rate || 0;
                    const consumptionValue = d.consumption || 0;
                    return d3.max([costValue / scaleFactor, rateValue / scaleFactor, consumptionValue]);
                },
                title: function(d) {
                    let standingChargeFraction = 0;
                    if (d.consumption !== null && d.consumption !== undefined) {
                        standingChargeFraction = standingChargeForDate(d.timestamp) / slotsPerDay;
                    }
                    const totalSlotCost = (d.cost || 0) + standingChargeFraction;
                    return [
                        `Time: ${d3.timeFormat("%H:%M")(d.timestamp)}`,
                        `Usage: ${(d.consumption || 0).toFixed(3)} kWh`,
                        `Unit Price: ${(d.rate || 0).toFixed(2)} p/kWh`,
                        `Usage Cost: ${(d.cost || 0).toFixed(2)} p`,
                        `Standing Charge: ${standingChargeFraction.toFixed(2)} p`,
                        `Total Cost: ${totalSlotCost.toFixed(2)} p`
                    ].join("\n");
                }
            })),
        ],
    });

    const chart = document.getElementById("chart");
    chart.style.cursor = 'crosshair';

    addStripes(plot);

    // Remove spinner if present
    const spinner = chart.querySelector(".spinner");
    if (spinner) spinner.remove();

    // Render plot into #plot div
    const plotDiv = document.getElementById("plot");
    if (plotDiv) {
        plotDiv.replaceChildren(plot);
    }

    // Calculate usage-only cost and standing charge over the window period
    const usageCost = data.reduce((sum, d) => sum + (d.cost || 0), 0);

    // Standing charge: sum apportioned to each visible slot
    // Group slots by day to be careful about fractional day floating point arithmetic
    const standingCharge = Array.from(d3.group(slotsWithConsumption, d => dayKeyUTC(d.timestamp)))
        .reduce((sum, [dayKey, slots]) => {
            const dailyStandingCharge = standingChargeMap.get(dayKey) || 0;
            return sum + (dailyStandingCharge * slots.length / slotsPerDay);
        }, 0);

    const totalCost = usageCost + standingCharge;

    // Calculate hours covered (based on actual data points)
    let period = 0;
    if (data.length > 0) {
        period = intervalHours + (data[data.length - 1].timestamp - data[0].timestamp) / (1000 * 60 * 60);
    }

    // Calculate average hourly cost
    let avgHourlyCost = 0;
    if (period > 0) {
        avgHourlyCost = totalCost / period;
    }

    // Calculate average price (independent of consumption)
    // This will vary depending on the time-of-use tariff, e.g. Octopus Agile.
    // For Fixed and Flexible tariffs, this is (mostly) constant.
    let avgPrice = 0;
    if (data.length > 0) {
        avgPrice = data.reduce((sum, d) => sum + (d.rate || 0), 0) / data.length;
    }

    // Calculate total consumption
    const totalConsumption = data.reduce((sum, d) => sum + (d.consumption || 0), 0);

    // Calculate average and max power (kW)
    let avgPower = 0;
    if (data.length > 0) {
        avgPower = data.reduce((sum, d) => sum + (d.consumption || 0), 0) / data.length / intervalHours;
    }

    let maxPower = 0;
    if (data.length > 0) {
        maxPower = Math.max(...data.map(d => (d.consumption || 0) / intervalHours));
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

    const costPriceElem = document.getElementById('val-cost-price');
    if (costPriceElem) costPriceElem.textContent = avgPrice.toFixed(2);

    const costUsageElem = document.getElementById('val-cost-usage');
    if (costUsageElem) costUsageElem.textContent = formatCost(usageCost);

    const costStandingChargeElem = document.getElementById('val-cost-standing-charge');
    if (costStandingChargeElem) costStandingChargeElem.textContent = formatCost(standingCharge);

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
