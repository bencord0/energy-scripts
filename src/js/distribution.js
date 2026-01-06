import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { initDatabase, getPriceDistribution, getStandingCharge, getSlotCountsByDay, getTimeWindow } from '/js/db.js';
import { priceColors, addStripes } from '/js/colors.js';
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

    const { data, timeWindow } = getPriceDistribution(db, startStr, endStr, 'IMPORT');
    const { slotsPerDay } = getTimeWindowInfo(timeWindow);

    const maxX = d3.max(data, d => d.rate) || 0;
    const ceilX = Math.ceil(maxX / 10) * 10;
    const interval = 0.5;

    // Standing charge total map and slot counts
    const standingChargeMap = getStandingCharge(db, startStr, endStr, 'IMPORT');
    const slotsByDay = getSlotCountsByDay(db, startStr, endStr, 'IMPORT');
    let slotBasedStandingCharge = 0;
    let totalSlotsPeriod = 0;
    for (const [day, standingCharge] of standingChargeMap.entries()) {
        const slots = slotsByDay.get(day) || 0;
        slotBasedStandingCharge += (standingCharge || 0) * (slots / slotsPerDay);
        totalSlotsPeriod += slots;
    }

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

    let maxY = 0;

    // Compute standing charge per slot across the period
    let standingChargePerSlot = 0;
    if (totalSlotsPeriod > 0) {
        standingChargePerSlot = slotBasedStandingCharge / totalSlotsPeriod;
    }

    // For each input bin, compute standing charge contribution based on slots in that bin
    inputs.forEach(d => {
        const standingChargeContribution = (d.slots || 0) * standingChargePerSlot;
        d.standing_charge = standingChargeContribution;
        d.cost_with_standing_charge = d.cost + standingChargeContribution;
    });

    // Recompute maxY to include stacked totals
    maxY = d3.max(inputs, d => d.cost_with_standing_charge) || d3.max(inputs, d => d.cost) || 0;

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
            Plot.rectY(inputs, {
                x1: "rate_start",
                x2: "rate_end",
                y: "standing_charge",
                fill: 'rate_mid',
                fillOpacity: 0.8,
                stroke: 'none'
            }),
            // Stripes overlay on standing charge (diagonal pattern)
            Plot.rectY(inputs, {
                x1: "rate_start",
                x2: "rate_end",
                y: "standing_charge",
                fill: 'url(#stripes)',
                fillOpacity: 1,
                stroke: 'none',
                mixBlendMode: 'multiply'
            }),
            // Usage bars stacked on top of standing charges
            Plot.rectY(inputs, {
                x1: "rate_start",
                x2: "rate_end",
                y1: "standing_charge",
                y2: "cost_with_standing_charge",
                fill: "rate_end",
                tip: true,
                title: d => `Rate: ${d.rate_start} - ${d.rate_end} p/kWh\nUsage: ${d.consumption.toFixed(3)} kWh\nUsage cost: ${formatCost(d.cost)}\nStanding Charge: ${formatCost(d.standing_charge)}\nTotal: ${formatCost(d.cost_with_standing_charge)}`
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

    addStripes(plot);

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

        // Limit of our data
        const earliestDay = new Date("2025-12-01");
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
