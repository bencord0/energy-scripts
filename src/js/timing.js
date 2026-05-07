import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { getConsumptionByTimeOfDay, getDataLimits } from '/js/db.js';
import { priceColors } from '/js/colors.js';
import { formatCost, getTimeWindowInfo } from '/js/utils.js';

const { earliestDate, latestDate } = await getDataLimits();

const urlParams = new URLSearchParams(window.location.search);

const millisecondsPerDay = 864e5;
const today = new Date().setHours(0, 0, 0, 0);
const yesterday = new Date(today - millisecondsPerDay);
const dayBefore = new Date(yesterday - millisecondsPerDay);
let startDate = new Date(urlParams.get('start') || dayBefore);
let endDate = new Date(urlParams.get('end') || today);

let urlDebouncer;
function updateUrl(startStr, endStr) {
    // Truncate to day
    startStr = startStr.slice(0, 10);
    endStr = endStr.slice(0, 10);

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

    let { data, timeWindow } = await getConsumptionByTimeOfDay(startStr, endStr);
    data = data.map((d) => {
        let [hours, minutes] = d.time_of_day.split(':').map(Number);
        let start = new Date();
        start.setHours(hours, minutes, 0, 0);
        d.timestamp = start;

        let end = new Date(d.timestamp);
        end.setMinutes(end.getMinutes() + 30);
        d.timestampEnd = end;

        return d;
    });

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
                y: d => d.consumption - d.generation,
                fill: d => (d.consumption - d.generation) > 0 ? d.import_rate : d.export_rate,
                reverse: true,
                tip: true,
                title: d => {
                    let consumption = d.consumption || 0;
                    let generation  = d.generation || 0;
                    return [
                        `Time of Day: ${d3.timeFormat("%H:%M")(d.timestamp)}`,
                        `Imported: ${consumption.toFixed(3)} kWh`,
                        `Import Price: ${d.import_rate.toFixed(2)} p/kWh`,
                        `Exported: ${generation.toFixed(3)} kWh`,
                        `Export Price: ${d.export_rate.toFixed(2)} p/kWh`,
                    ].join("\n");
                },

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

await render();

let pendingUpdate = false;
window.addEventListener('wheel', (e) => {
    e.preventDefault();

    if (e.deltaY !== 0) {
        startDate.setDate(startDate.getDate() + (e.deltaY / 10));
        if (startDate > dayBefore) {
            startDate.setTime(dayBefore.getTime());
        }

        // Limit to available data range
        if (earliestDate && startDate < earliestDate) {
            startDate.setTime(earliestDate.getTime());
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
