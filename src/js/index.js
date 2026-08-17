import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { getConsumption, getStandingCharge, getAgilePredictions } from '/js/db.js';
import { priceColors, addStripes } from '/js/colors.js';
import { formatCost, getTimeWindowInfo } from '/js/utils.js';

const urlParams = new URLSearchParams(window.location.search);

const millisecondsPerDay = 864e5;
const today = new Date().setHours(0, 0, 0, 0);
const tomorrow = new Date(today + millisecondsPerDay);
const yesterday = new Date(today - millisecondsPerDay);
const dayBefore = new Date(yesterday - millisecondsPerDay);
let startDate = new Date(urlParams.get('start') || dayBefore);
let endDate = new Date(urlParams.get('end') || tomorrow);

let urlDebouncer;
function updateUrl(startStr, endStr, timeWindow) {
    if (timeWindow === '1d') {
        startStr = startStr.slice(0, 10);
        endStr = endStr.slice(0, 10);
    } else if (timeWindow === '1h') {
        startStr = startStr.slice(0, 13) + ':00';
        endStr = endStr.slice(0, 13) + ':00';
    }

    if (startStr.endsWith('T00:00')) startStr = startStr.slice(0, 10);
    if (endStr.endsWith('T00:00')) endStr = endStr.slice(0, 10);

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

async function render() {
    // https://observablehq.com/blog/reshaping-data-plot-d3
    // https://r4ds.had.co.nz/tidy-data.html
    // Expect data in a "tidy" format.
    const startStr = startDate.toISOString().slice(0, 16);
    const endStr = endDate.toISOString().slice(0, 16);

    const durationHours = (endDate - startDate) / (1000 * 60 * 60);

    let timeWindow;
    let data;
    let standingCharges;
    let pricePredictions;

    await Promise.all([
        getConsumption(startStr, endStr).then((result) => {
            timeWindow = result.timeWindow;
            data = result.data;
        }),

        getStandingCharge(startStr, endStr, 'IMPORT').then(result => {
            standingCharges = result;
        }),

        getAgilePredictions(startStr, endStr, 'A').then(result => {
            pricePredictions = result;
        }),
    ]);

    data = data.map(d => {
        const timestamp = new Date(d.timestamp);
        return {
            ...d,
            timestamp,
        };
    });

    const { slotsPerDay, intervalHours } = getTimeWindowInfo(timeWindow);
    const interval = {
        "1d": d3.timeDay,
        "1h": d3.timeHour,
        "30m": d3.timeMinute.every(30),
    }[timeWindow];

    function dayKeyUTC(d) {
        let date = new Date(d);
        return `${date.getUTCFullYear()}-${String(date.getUTCMonth() + 1).padStart(2, '0')}-${String(date.getUTCDate()).padStart(2, '0')}`;
    }

    function standingChargeForDate(date) {
        const key = dayKeyUTC(date);
        let charge = 0;
        standingCharges.map(({day, daily_standing_charge}) => {
            if (day == key) {
                charge = daily_standing_charge;
            }
        });

        return charge;
    }

    // Compute maxP considering standing charge and export rates
    const maxP = 100;
    const maxKWh = 10;
    const scaleFactor = maxP / maxKWh;

    // Precompute standing charge and usage stacked above the standing charge bar
    // Only include rows for slots where we have consumption data
    const slotsWithConsumption = data.filter(d => d.consumption > 0);
    const slotsWithGeneration = data.filter(d => d.generation > 0);

    const standingChargeRows = slotsWithConsumption.map(d => {
        const timestamp = new Date(d.timestamp);
        const standingChargePence = standingChargeForDate(timestamp) / slotsPerDay;
        return {
            timestamp,
            import_rate: d.import_rate,
            y2: (standingChargePence) / scaleFactor,
            hasUsage: (d.consumption || 0) > 0,
        };
    });

    const usageRows = slotsWithConsumption.map(d => {
        const timestamp = new Date(d.timestamp);
        const standingChargePence = standingChargeForDate(timestamp) / slotsPerDay;
        const standingChargeScaled = standingChargePence / scaleFactor;
        const usageScaled = (d.cost || 0) / scaleFactor;
        return {
            timestamp,
            import_rate: d.import_rate,
            export_rate: d.export_rate,
            y1: standingChargeScaled,
            y2: standingChargeScaled + usageScaled,
        };
    });

    // Compute dynamic Y-axis domain: always show at least 10 kWh / £1, but expand if data exceeds it
    const dataMaxY = Math.max(
        ...data.map(d => d.consumption || 0),
        ...data.map(d => d.solar_generation || 0),
        ...data.map(d => d.charge || 0),
        ...data.map(d => d.car_charge || 0),
        ...usageRows.map(d => d.y2 || 0),
        ...data.map(d => d.cost / scaleFactor || 0),
        ...data.map(d => d.import_rate / scaleFactor || 0),
    );
    const dataMinY = Math.max(
        ...data.map(d => d.generation || 0),
        ...data.map(d => d.discharge || 0),
        ...data.map(d => d.sale / scaleFactor || 0),
        ...data.map(d => d.export_rate / scaleFactor || 0),
    );
    const yMax = Math.max(maxKWh, dataMaxY * 1.05); // 5% headroom
    const yMin = Math.max(0, dataMinY * 1.05);

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

    // Calculate available dimensions accounting for summary box and margins
    const summaryElement = document.getElementById('cost-summary');
    const bodyMargin = 40; // body margin (20px * 2)
    const chartPadding = 20; // additional padding for chart

    // Get the summary box height (it's either overlaid or stacked depending on viewport)
    let summaryHeight = 0;
    if (summaryElement && window.innerWidth <= 600) {
        // On mobile, summary is stacked above the chart
        summaryHeight = summaryElement.getBoundingClientRect().height + 8; // +8 for margin
    }

    const availableHeight = window.innerHeight - bodyMargin - summaryHeight - chartPadding;
    const availableWidth = window.innerWidth - bodyMargin;

    // Calculate appropriate tick count based on screen width
    // Approximate label width: ~50px for dates, ~40px for times
    const labelWidth = durationHours > 48 ? 50 : 40;
    const maxTicks = Math.max(4, Math.floor(availableWidth / (labelWidth + 10)));
    const tickCount = Math.min(12, maxTicks);

    const plot = Plot.plot({
        height: Math.max(200, availableHeight),
        width: Math.max(200, availableWidth),
        x: {
            type: "time",
            label: "timestamp",
            tickFormat: formatTick,
            ticks: tickCount,
            domain: [startDate, endDate],
        },
        y: {
            grid: true,
            zero: true,
            domain: [-yMin, yMax],
        },
        color: priceColors,
        marks: [
            // Standing charge overlay
            Plot.rectY(standingChargeRows, {
                x: 'timestamp',
                y: 'y2',
                interval: interval,
                fill: d => d.hasUsage ? d.import_rate : '#e0e0e0',
                fillOpacity: d => d.hasUsage ? 0.22 : 0.6,
                inset: 0,
                shapeRendering: "crispEdges",
            }),
            Plot.rectY(standingChargeRows, {
                x: 'timestamp',
                y: 'y2',
                interval: interval,
                fill: 'url(#stripes)',
                fillOpacity: 1,
                stroke: 'none',
                mixBlendMode: 'multiply',
                inset: 0,
                shapeRendering: "crispEdges",
            }),
            // Imported Usage stacked above standing charge using y1/y2
            Plot.rectY(usageRows, {
                x: 'timestamp',
                y1: 'y1',
                y2: 'y2',
                interval: interval,
                fill: 'import_rate',
                inset: 0,
                shapeRendering: "crispEdges",
            }),
            // Exported Usage
            Plot.rectY(data, {
                x: 'timestamp',
                y: d => -(d.sale || 0) / scaleFactor,
                interval: interval,
                fill: 'export_rate',
                inset: 0,
                shapeRendering: "crispEdges",
            }),
            // Import Price
            Plot.rectY(data, {
                x: 'timestamp',
                y: d => d.import_rate / scaleFactor,
                interval: interval,
                fill: "#ccc",
                fillOpacity: 0.4,
                mixBlendMode: "multiply",
                inset: 0,
                shapeRendering: "crispEdges",
            }),
            // Export Price
            Plot.rectY(data, {
                x: 'timestamp',
                y: d => -d.export_rate / scaleFactor,
                interval: interval,
                fill: "#ccc",
                fillOpacity: 0.4,
                mixBlendMode: "multiply",
                inset: 0,
                shapeRendering: "crispEdges",
            }),
            // Grid Consumption
            Plot.lineY(data, {
                x: 'timestamp',
                y: 'consumption',
                stroke: "rgba(0, 127, 200, 0.8)",
                strokeWidth: 1,
                strokeDasharray: "5,1",
                curve: "step-after",
            }),
            // Grid Export
            Plot.lineY(data, {
                x: 'timestamp',
                y: d => -d.generation,
                stroke: "rgba(0, 127, 200, 0.8)",
                strokeWidth: 1,
                strokeDasharray: "5,1",
                curve: "step-after",
            }),
            // Battery Charge
            Plot.lineY(data, {
                x: 'timestamp',
                y: 'charge',
                stroke: "rgba(0, 240, 45, 0.8)",
                strokeWidth: 1,
                strokeDasharray: "5,1",
                curve: "step-after",
            }),
            // Battery Discharge
            Plot.lineY(data, {
                x: 'timestamp',
                y: d => -d.discharge,
                stroke: "rgba(0, 240, 45, 0.8)",
                strokeWidth: 1,
                strokeDasharray: "5,1",
                curve: "step-after",
            }),
            // Solar Generation
            Plot.lineY(data, {
                x: 'timestamp',
                y: 'solar_generation',
                stroke: "rgba(255, 200, 0, 0.8)",
                strokeWidth: 1,
                curve: "step-after",
            }),
            // Car Charge
            Plot.lineY(data, {
                x: 'timestamp',
                   // visually stack ontop of battery charge
                y: d => d.car_charge > 0 ? d.charge + d.car_charge : 0,
                stroke: "rgba(150, 10, 200, 0.4)",
                strokeWidth: 1,
                curve: "step-after",
            }),
            // AgilePredict - https://agilepredict.com/api_how_to
            Plot.lineY(pricePredictions, {
                x: 'timestamp',
                y: d => d.import_prediction / scaleFactor,
                stroke: "rgba(50, 50, 50, 0.4)",
                strokeWidth: 1,
                curve: "step-after",
            }),
            Plot.lineY(pricePredictions, {
                x: 'timestamp',
                y: d => -d.export_prediction / scaleFactor,
                stroke: "rgba(50, 50, 50, 0.4)",
                strokeWidth: 1,
                curve: "step-after",
            }),
            // Baseline at zero to anchor bars
            Plot.ruleY([0]),
            Plot.axisY({ anchor: "left", label: "Used Energy (kWh)" }),
            Plot.axisY({
                anchor: "right",
                label: "price (p/kWh), cost (p)",
                tickFormat: y => formatCost((y * scaleFactor).toFixed(0)),
            }),
            // Current Time Marker
            Plot.ruleX([new Date()], {
                stroke: "red",
                strokeWidth: 2,
                strokeDasharray: "4,4"
            }),
            // Tooltip with pointerX for time-series tracking
            Plot.tip(data, Plot.pointerX({
                x: "timestamp",
                y: d => {
                    const up = Math.max(d.consumption || 0, d.charge || 0, d.solar_generation || 0);
                    const down = Math.max(d.generation || 0, d.discharge || 0);
                    return up - down;
                },
                title: d => {
                    const timestamp = new Date(d.timestamp);
                    let standingChargeFraction = 0;
                    if (d.consumption !== null && d.consumption !== undefined) {
                        standingChargeFraction = standingChargeForDate(timestamp) / slotsPerDay;
                    }
                    const totalSlotCost = (d.cost || 0) + standingChargeFraction - (d.sale || 0);
                    return [
                        `Time: ${d3.timeFormat("%H:%M")(timestamp)}`,
                        `Solar: ${(d.solar_generation || 0).toFixed(2)} kWh`,
                        `Import: ${(d.consumption || 0).toFixed(3)} kWh`,
                        `Import Price: ${(d.import_rate || 0).toFixed(2)} p/kWh`,
                        `Import Cost: ${formatCost((d.cost || 0))}`,
                        `Battery Charge: ${(d.charge || 0).toFixed(2)} kWh`,
                        `Battery Disharge: ${(d.discharge || 0).toFixed(2)} kWh`,
                        `Car Charge: ${(d.car_charge || 0).toFixed(2)} kWh`,
                        `Export: ${(d.generation || 0).toFixed(3)} kWh`,
                        `Export Price: ${(d.export_rate || 0).toFixed(2)} p/kWh`,
                        `Export Sale: ${formatCost((d.sale || 0))}`,
                        `Standing Charge: ${standingChargeFraction.toFixed(2)} p`,
                        `Total Cost: ${formatCost(totalSlotCost)}`,
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
    const standingCharge = Array.from(d3.group(slotsWithConsumption, d => dayKeyUTC(new Date(d.timestamp))))
        .reduce((sum, [dayKey, slots]) => {
            const dailyStandingCharge = standingChargeForDate(dayKey);
            return sum + (dailyStandingCharge * slots.length / slotsPerDay);
        }, 0);

    const totalCost = usageCost + standingCharge;

    // Calculate total sale (export)
    const totalSale = data.reduce((sum, d) => sum + (d.sale || 0), 0);

    // Calculate net cost
    const netCost = totalCost - totalSale;

    // Calculate hours covered (based on actual data points)
    let period = 0;
    if (data.length > 0) {
        const timeFrom = new Date(data[0].timestamp);
        const timeTo = new Date(data[data.length - 1].timestamp);
        period = intervalHours + (timeTo - timeFrom) / (1000 * 60 * 60);
    }

    // Calculate total consumption
    const totalConsumption = data.reduce((sum, d) => sum + (d.consumption || 0), 0);

    // Calculate total exported (generation)
    const totalExported = data.reduce((sum, d) => sum + (d.generation || 0), 0);

    // Calculate average and max power (kW)
    let avgPower = 0;
    if (data.length > 0) {
        avgPower = data.reduce((sum, d) => sum + (d.consumption || 0), 0) / data.length / intervalHours;
    }

    let maxPower = 0;
    if (data.length > 0) {
        maxPower = Math.max(...data.map(d => (d.consumption || 0) / intervalHours));
    }

    // Calculate average prices (independent of consumption)
    let avgPrice = 0;
    let avgSalePrice = 0;
    if (data.length > 0) {
        avgPrice = data.reduce((sum, d) => sum + (d.import_rate || 0), 0) / data.length;
        avgSalePrice = data.reduce((sum, d) => sum + (d.export_rate || 0), 0) / data.length;
    }

    // Calculate effective prices (averages that account for consumption)
    let effectivePrice = usageCost / totalConsumption;
    if (!Number.isFinite(effectivePrice)) effectivePrice = 0;
    let effectiveSalePrice = totalSale / totalExported;
    if (!Number.isFinite(effectiveSalePrice)) effectiveSalePrice = 0;

    // Calculate Solar Generation
    const totalGenerated = data.reduce((sum, d) => sum + (d.solar_generation || 0), 0);

    // Calculate Battery Charge/Discharge
    const totalCharged = data.reduce((sum, d) => sum + (d.charge || 0), 0);
    const totalDischarged = data.reduce((sum, d) => sum + (d.discharge || 0), 0);

    // Car Charging
    const totalCarCharged = data.reduce((sum, d) => sum + (d.car_charge || 0), 0);

    // Update Cost Summary Values in DOM
    const periodElem = document.getElementById('val-period');
    if (periodElem) periodElem.textContent = period.toFixed(1);

    const powerTotalElem = document.getElementById('val-power-total');
    if (powerTotalElem) powerTotalElem.textContent = totalConsumption.toFixed(2);

    const powerAvgElem = document.getElementById('val-power-avg');
    if (powerAvgElem) powerAvgElem.textContent = avgPower.toFixed(3);

    const powerMaxElem = document.getElementById('val-power-max');
    if (powerMaxElem) powerMaxElem.textContent = maxPower.toFixed(3);

    // Cost section
    const costTotalElem = document.getElementById('val-cost-total');
    if (costTotalElem) costTotalElem.innerHTML = formatCost(totalCost);

    const effcPriceElem = document.getElementById('val-cost-effc');
    if (effcPriceElem) effcPriceElem.textContent = effectivePrice.toFixed(2);
    const costPriceElem = document.getElementById('val-cost-price');
    if (costPriceElem) costPriceElem.textContent = avgPrice.toFixed(2);

    const costUsageElem = document.getElementById('val-cost-usage');
    if (costUsageElem) costUsageElem.textContent = formatCost(usageCost);

    const costStandingChargeElem = document.getElementById('val-cost-standing-charge');
    if (costStandingChargeElem) costStandingChargeElem.textContent = formatCost(standingCharge);

    // Export section
    const saleTotalElem = document.getElementById('val-sale-total');
    if (saleTotalElem) saleTotalElem.innerHTML = formatCost(totalSale);

    const powerExportedElem = document.getElementById('val-power-exported');
    if (powerExportedElem) powerExportedElem.textContent = totalExported.toFixed(2);

    const saleEffcElem = document.getElementById('val-sale-effc');
    if (saleEffcElem) saleEffcElem.textContent = effectiveSalePrice.toFixed(2);
    const salePriceElem = document.getElementById('val-sale-price');
    if (salePriceElem) salePriceElem.textContent = avgSalePrice.toFixed(2);

    // Summary section
    const netCostElem = document.getElementById('val-net-cost');
    if (netCostElem) netCostElem.innerHTML = formatCost(netCost);

    // Solar / Battery section
    const solarGenElem = document.getElementById('val-solar-gen');
    if (solarGenElem) solarGenElem.innerHTML = totalGenerated.toFixed(2);

    const battChargedElem = document.getElementById('val-batt-charged');
    if (battChargedElem) battChargedElem.innerHTML = totalCharged.toFixed(2);

    const battDischargedElem = document.getElementById('val-batt-discharged');
    if (battDischargedElem) battDischargedElem.innerHTML = totalDischarged.toFixed(2);

    const carChargedElem = document.getElementById('val-car-charged');
    if (carChargedElem) carChargedElem.innerHTML = totalCarCharged.toFixed(2);

    // Update URL without refreshing (Debounced)
    updateUrl(startStr, endStr, timeWindow);
}

// Initial render
await render();

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
            render().then(() => {
                pendingUpdate = false;
            });
        });
    }
}, { passive: false });

window.addEventListener('resize', () => {
    if (!pendingUpdate) {
        pendingUpdate = true;
        requestAnimationFrame(() => {
            render().then(() => {
                pendingUpdate = false;
            });
        });
    }
});

// Touch handling state
let activeTouches = new Map(); // Track all active touches by identifier
let lastPanX = null;
let lastPinchDist = null;
let lastPinchCenter = null;
let isGestureActive = false;

const chartElement = document.getElementById('chart');
const costSummary = document.getElementById('cost-summary');

// Helper to check if a touch started on the summary box
function isTouchOnSummary(touch) {
    if (!costSummary) return false;
    const rect = costSummary.getBoundingClientRect();
    return (
        touch.clientX >= rect.left &&
        touch.clientX <= rect.right &&
        touch.clientY >= rect.top &&
        touch.clientY <= rect.bottom
    );
}

// Helper to get distance between two touches
function getTouchDistance(t1, t2) {
    return Math.hypot(t2.clientX - t1.clientX, t2.clientY - t1.clientY);
}

// Helper to get center point between two touches
function getTouchCenter(t1, t2) {
    return (t1.clientX + t2.clientX) / 2;
}

// Reset all touch tracking state
function resetTouchState() {
    activeTouches.clear();
    lastPanX = null;
    lastPinchDist = null;
    lastPinchCenter = null;
    isGestureActive = false;
}

chartElement.addEventListener('touchstart', (e) => {
    // Track all new touches, filtering out those on summary
    for (const touch of e.changedTouches) {
        if (!isTouchOnSummary(touch)) {
            activeTouches.set(touch.identifier, {
                startX: touch.clientX,
                startY: touch.clientY,
                currentX: touch.clientX,
                currentY: touch.clientY
            });
        }
    }

    // Initialize gesture based on active touch count
    const validTouches = Array.from(activeTouches.keys());

    if (validTouches.length === 1) {
        const touchData = activeTouches.get(validTouches[0]);
        lastPanX = touchData.currentX;
        lastPinchDist = null;
        lastPinchCenter = null;
        isGestureActive = true;
    } else if (validTouches.length >= 2) {
        // Get the first two valid touches from the event
        const allTouches = Array.from(e.touches);
        const validTouchPairs = allTouches.filter(t => activeTouches.has(t.identifier));

        if (validTouchPairs.length >= 2) {
            const t1 = validTouchPairs[0];
            const t2 = validTouchPairs[1];
            lastPinchDist = getTouchDistance(t1, t2);
            lastPinchCenter = getTouchCenter(t1, t2);
            lastPanX = null; // Disable pan when pinching
            isGestureActive = true;
        }
    }
}, { passive: true });

chartElement.addEventListener('touchmove', (e) => {
    if (!isGestureActive || activeTouches.size === 0) return;

    // Update tracked touch positions
    for (const touch of e.changedTouches) {
        if (activeTouches.has(touch.identifier)) {
            const data = activeTouches.get(touch.identifier);
            data.currentX = touch.clientX;
            data.currentY = touch.clientY;
        }
    }

    // Get valid touches from current event
    const allTouches = Array.from(e.touches);
    const validTouches = allTouches.filter(t => activeTouches.has(t.identifier));

    if (validTouches.length === 0) return;

    // Prevent default to stop browser scrolling/zooming
    if (e.cancelable) e.preventDefault();

    const rect = chartElement.getBoundingClientRect();
    const duration = endDate.getTime() - startDate.getTime();

    if (validTouches.length === 1 && lastPanX !== null) {
        // Single finger pan
        const currentX = validTouches[0].clientX;
        const deltaX = lastPanX - currentX;

        // Apply pan - minimum threshold to avoid jitter
        if (Math.abs(deltaX) > 1) {
            const shift = (deltaX / rect.width) * duration;
            startDate = new Date(startDate.getTime() + shift);
            endDate = new Date(endDate.getTime() + shift);
            lastPanX = currentX;
        }

    } else if (validTouches.length >= 2 && lastPinchDist !== null) {
        // Pinch zoom
        const t1 = validTouches[0];
        const t2 = validTouches[1];
        const currentDist = getTouchDistance(t1, t2);
        const currentCenter = getTouchCenter(t1, t2);

        // Minimum distance threshold to avoid division issues
        if (currentDist > 10 && lastPinchDist > 10) {
            const factor = lastPinchDist / currentDist;

            // Apply limits and deadzone
            if (Math.abs(factor - 1) > 0.01) {
                const newDuration = Math.max(
                    1000 * 60 * 30,
                    Math.min(duration * factor, 1000 * 60 * 60 * 24 * 365)
                );

                // Calculate focus point relative to chart width
                const mouseX = currentCenter - rect.left;
                const mouseRatio = Math.max(0, Math.min(1, mouseX / rect.width));
                const focusTime = startDate.getTime() + duration * mouseRatio;

                startDate = new Date(focusTime - newDuration * mouseRatio);
                endDate = new Date(focusTime + newDuration * (1 - mouseRatio));
            }
        }

        lastPinchDist = currentDist;
        lastPinchCenter = currentCenter;
    }

    if (!pendingUpdate) {
        pendingUpdate = true;
        requestAnimationFrame(() => {
            render().then(() => {
                pendingUpdate = false;
            });
        });
    }
}, { passive: false });

chartElement.addEventListener('touchend', (e) => {
    // Remove ended touches from tracking
    for (const touch of e.changedTouches) {
        activeTouches.delete(touch.identifier);
    }

    // Get remaining valid touches
    const allTouches = Array.from(e.touches);
    const validTouches = allTouches.filter(t => activeTouches.has(t.identifier));

    if (validTouches.length === 0) {
        // All touches ended
        resetTouchState();
    } else if (validTouches.length === 1) {
        // Transition from pinch to pan
        lastPanX = validTouches[0].clientX;
        lastPinchDist = null;
        lastPinchCenter = null;
    } else if (validTouches.length >= 2) {
        // Still pinching with remaining fingers
        const t1 = validTouches[0];
        const t2 = validTouches[1];
        lastPinchDist = getTouchDistance(t1, t2);
        lastPinchCenter = getTouchCenter(t1, t2);
        lastPanX = null;
    }
}, { passive: true });

// Handle touch cancel (e.g., incoming call, gesture interrupted)
chartElement.addEventListener('touchcancel', (e) => {
    // Remove cancelled touches
    for (const touch of e.changedTouches) {
        activeTouches.delete(touch.identifier);
    }

    // If no touches remain, reset completely
    if (activeTouches.size === 0) {
        resetTouchState();
    }
}, { passive: true });

// Update every minute
setInterval(() => {
    if (!pendingUpdate) {
        pendingUpdate = true;
        requestAnimationFrame(() => {
            render().then(() => {
                pendingUpdate = false;
            });
        });
    }
}, 60 * 1000);
