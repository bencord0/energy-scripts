import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { initDatabase, getTimingByTimeOfDay } from '/js/db.js';
import { priceColors } from '/js/colors.js';

const { sqlite3, db } = await initDatabase();
window.sqlite3 = sqlite3; // for debugging
window.db = db; // for debugging

const urlParams = new URLSearchParams(window.location.search);

const millisecondsPerDay = 864e5;
const today = new Date().setHours(0, 0, 0, 0);
const yesterday = new Date(today - millisecondsPerDay);
const dayBefore = new Date(yesterday - millisecondsPerDay);
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

    const data = getTimingByTimeOfDay(db, startStr, endStr, 'IMPORT');

    const maxConsumption = d3.max(data, d => d.consumption) || 1;

    const plot = Plot.plot({
        height: window.innerHeight - 40,
        width: window.innerWidth - 40,
        x: {
            type: "time",
            label: "Time of Day",
            tickFormat: d3.timeFormat("%H:%M"),
            ticks: 24,
        },
        y: {
            grid: true,
            label: "Total Energy Usage (kWh)",
        },
        color: priceColors,
        marks: [
            Plot.rectY(data, Plot.stackY({
                x1: "timestamp",
                x2: "timestampEnd",
                y: "consumption",
                fill: "rate",
                order: "rate",
                reverse: true,
                tip: true,
                title: d => `Time of Day: ${d3.timeFormat("%H:%M")(d.timestamp)}\nPrice: ${d.rate.toFixed(2)} p/kWh\nTotal Energy Usage: ${d.consumption.toFixed(3)} kWh`,
            })),
            Plot.ruleY([0]),
        ],
    });

    const plotElement = document.getElementById('plot');
    plotElement.replaceChildren(plot);

    // Remove spinner if present
    const chart = document.getElementById('chart');
    const spinner = chart.querySelector('.spinner');
    if (spinner) spinner.remove();

    // Update URL without refreshing
    updateUrl(startStr, endStr);
}

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
