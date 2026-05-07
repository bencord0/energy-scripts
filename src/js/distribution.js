import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { initDatabase, getPriceDistribution, getStandingCharge, getSlotCountsByDay, getTimeWindow } from '/js/db.js';
import { priceColors, addStripes } from '/js/colors.js';
import { importThresholds, exportThresholds } from '/js/thresholds.js';
import { formatCost, getTimeWindowInfo } from '/js/utils.js';

const { sqlite3, db } = await initDatabase();
window.sqlite3 = sqlite3; // for debugging
window.db = db; // for debugging

const urlParams = new URLSearchParams(window.location.search);

const millisecondsPerDay = 864e5;
let today = new Date().setHours(0, 0, 0, 0);
let yesterday = new Date(today - millisecondsPerDay);
let dayBefore = new Date(yesterday - millisecondsPerDay);
let startDate = new Date(urlParams.get('start') || dayBefore);
let endDate = new Date(urlParams.get('end') || today);

let urlDebouncer;
function updateUrl(startStr, endStr) {
    if (startStr.endsWith('T00:00')) startStr = startStr.slice(0, 10);
    if (endStr.endsWith('T00:00')) endStr = endStr.slice(0, 10);

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

    const { data, timeWindow } = await getPriceDistribution(db, startStr, endStr);
    const slotsPerDay = 48;

    const maxX = d3.max(data, d => Math.max(d.import_rate, d.export_rate)) || 0;
    const ceilX = Math.ceil(maxX / 10) * 10;
    const interval = 0.5;

    // Standing charge total map and slot counts
    const standingChargeMap = getStandingCharge(db, startStr, endStr, 'IMPORT');
    const slotsByDay = getSlotCountsByDay(db, startStr, endStr, 'IMPORT');
    let slotBasedStandingCharge = 0;
    let totalSlotsPeriod = 0;
    for (const [day, standingCharge] of standingChargeMap.entries()) {
        const slots = slotsByDay.get(day) || 0;
        const slotStandingCharge = (standingCharge || 0) * (slots / slotsPerDay);
        slotBasedStandingCharge += slotStandingCharge;
        totalSlotsPeriod += slots;
    }

    // 1. Pre-calculate bins
    const thresholds = d3.range(0, ceilX + interval, interval);
    const importBinFn = d3.bin()
        .value(d => d.import_rate)
        .domain([0, ceilX])
        .thresholds(thresholds);
    const exportBinFn = d3.bin()
        .value(d => d.export_rate)
        .domain([0, ceilX])
        .thresholds(thresholds);

    const importBins = importBinFn(data).map(bin => {
        // bin is an array of data points, with x0 and x1
        const consumption = d3.sum(bin, d => d.consumption);
        const cost = d3.sum(bin, d => d.cost);
        const slotsCount = d3.sum(bin, d => d.slots || 0);
        return {
            rate_start: bin.x0,
            rate_end: bin.x1,
            rate_mid: (bin.x0 + bin.x1) / 2,
            consumption: consumption,
            cost: cost,
            slots: slotsCount
        };
    }).filter(d => d.consumption > 0);

    const exportBins = exportBinFn(data).map(bin => {
        const generation = d3.sum(bin, d => d.generation);
        const sale = d3.sum(bin, d => d.sale);
        return {
            rate_start: bin.x0,
            rate_end: bin.x1,
            generation: generation,
            sale: sale,
        };
    }).filter(d => d.generation > 0);

    let maxY = 0;
    let minY = 0;

    // Compute standing charge per slot across the period
    let standingChargePerSlot = 0;
    if (totalSlotsPeriod > 0) {
        standingChargePerSlot = slotBasedStandingCharge / totalSlotsPeriod;
    }

    // For each input bin, compute standing charge contribution based on slots in that bin
    importBins.forEach(d => {
        const standingChargeContribution = (d.slots || 0) * standingChargePerSlot;
        d.standing_charge = standingChargeContribution;
        d.cost_with_standing_charge = d.cost + standingChargeContribution;
    });

    // Recompute maxY and minY to include stacked totals
    maxY = d3.max(importBins, d => d.cost_with_standing_charge) || d3.max(importBins, d => d.cost) || 0;
    minY = -d3.max(exportBins, d => d.sale) || 0;

    // 2. Generate Iso-Usage Lines
    // Cost (y) = Usage (m) * Rate (x)
    // These are straight lines y = mx
    // Find roughly the max usage in a bin to determine steps
    const maxBarUsage = d3.max(importBins, d => d.consumption) || 0;

    // Determine order of magnitude for steps
    const stepUsage = Math.pow(10, Math.floor(Math.log10(maxBarUsage || 1)));
    const isoUsage = [];

    // Generate 1x, 2x, 5x, 10x steps
    [0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 75, 100].forEach(m => {
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
            domain: [minY, maxY * 1.1], // give some headroom
        },
        color: priceColors,
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

            // Standing charges
            Plot.rectY(importBins, {
                x1: "rate_start",
                x2: "rate_end",
                y: "standing_charge",
                fill: 'rate_mid',
                fillOpacity: 0.8,
                stroke: 'none'
            }),
            // Stripes overlay on standing charge (diagonal pattern)
            Plot.rectY(importBins, {
                x1: "rate_start",
                x2: "rate_end",
                y: "standing_charge",
                fill: 'url(#stripes)',
                fillOpacity: 1,
                stroke: 'none',
                mixBlendMode: 'multiply'
            }),
            // Import bars stacked on top of standing charges
            Plot.rectY(importBins, {
                x1: "rate_start",
                x2: "rate_end",
                y1: "standing_charge",
                y2: "cost_with_standing_charge",
                fill: "rate_end",
            }),
            // Export bars go below the x-axis
            Plot.rectY(exportBins, {
                x1: "rate_start",
                x2: "rate_end",
                y: d => -d.sale,
                fill: "rate_end",
            }),
            Plot.axisY({
                anchor: "left",
                label: "Cost",
                tickFormat: (d) => formatCost(d),
            }),
            // Base line at Y=0
            Plot.ruleY([0]),
            // Representative lines of common tariffs
            Plot.ruleX(importThresholds, {
                x: "value",
                y1: 0,
                y2: maxY * 0.8,
                stroke: "color",
                strokeWidth: 1,
                strokeDasharray: "4,4",
            }),
            Plot.text(importThresholds, {
                x: "value",
                y: maxY * 1,
                text: d => `${d.name} (${d.value}p)`,
                fill: "color",
                rotate: -90,
                dy: -4,
                textAnchor: "end",
                fontSize: 6,
            }),
            //Plot.ruleX(exportThresholds, {
            //    x: "value",
            //    y1: minY * 0.5,
            //    y2: 0,
            //    stroke: "color",
            //    strokeWidth: 1,
            //    strokeDasharray: "4,4",
            //}),
            //Plot.text(exportThresholds, {
            //    x: "value",
            //    y: minY,
            //    text: d => `${d.name} (${d.value}p)`,
            //    fill: "color",
            //    rotate: -90,
            //    dy: -4,
            //    textAnchor: "start",
            //    fontSize: 6,
            //}),
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

    addStripes(plot);

    // Update URL without refreshing
    updateUrl(startStr, endStr);
}

// Initial render
await render();

let pendingUpdate = false;
window.addEventListener('wheel', (e) => {
    e.preventDefault();

    if (e.deltaY !== 0) {
        startDate.setHours(startDate.getHours() + e.deltaY);
        if (startDate > dayBefore) {
            startDate.setTime(dayBefore.getTime());
        }

        // Limit of our data
        const earliestDay = new Date("2025-12-01");
        if (startDate < earliestDay) {
            startDate.setTime(earliestDay.getTime());
        }
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
