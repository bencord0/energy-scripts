import * as d3 from '/js/d3.esm.min.js';
import * as Plot from '/js/plot.esm.min.js';
import { default as sqlite3WasmInit } from '/js/sqlite-wasm-3510100/jswasm/sqlite3.mjs';

// https://observablehq.com/blog/reshaping-data-plot-d3
// https://r4ds.had.co.nz/tidy-data.html
// Expect data in a "tidy" format.
var data = [];

const [sqlite3, arrayBuffer] = await Promise.all([
    sqlite3WasmInit(),
    fetch('/data/power.sqlite3').then(res => res.arrayBuffer())
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

try {
    db.exec({
        sql: `SELECT
            c.interval_start,
            c.consumption,
            r.value,
            (c.consumption * r.value) as cost
        FROM consumption as c
        LEFT JOIN tariff_rates as r
        ON c.interval_start = r.valid_from
        ORDER BY c.interval_start ASC
        `,
        callback: (row) => {
            //console.log({row});
            let timestamp = new Date(row[0]);
            let datum = {
                timestamp,
                consumption: row[1],
                rate: row[2],
                cost: row[3],
            };
            //console.log({row, datum});
            data.push(datum);
        },
    });
} finally {
    db.close();
}

//console.log({data});
const maxKWh = d3.max(data, d => d.consumption) || 1;
const maxP = d3.max(data, d => Math.max(d.rate, d.cost)) || 40;
const scaleFactor = maxP / maxKWh;

let consumptionBar = Plot.lineY(data, {
    x: 'timestamp',
    y: 'consumption',
    stroke: "rgba(0, 127, 200, 0.8)",
});
let rateBar = Plot.rectY(data, {
    x: 'timestamp',
    y: d => d.rate / scaleFactor,
    interval: d3.timeMinute.every(30),
    fill: "rgba(255, 127, 0, 0.2)",
    mixBlendMode: "multiply",
});
let costBar = Plot.rectY(data, {
    x: 'timestamp',
    y: d => d.cost / scaleFactor,
    interval: d3.timeMinute.every(30),
    fill: 'rate',
    mixBlendMode: "multiply",
});

function formatTick(d) {
    if (d.getHours() === 0) {
        if (d.getMonth() === 0 && d.getDate() === 1) {
            return d3.timeFormat("%Y")(d);
        }
        return d3.timeFormat("%b %d")(d);
    }
    return d3.timeFormat("%H:%M")(d);
}

const plot = Plot.plot({
    width: window.innerWidth - 40,
    x: {
        type: "time",
        label: "timestamp",
        tickFormat: formatTick,
        ticks: d3.timeHour.every(6),
    },
    y: { grid: true },
    marks: [
        rateBar,
        costBar,
        consumptionBar,
        Plot.axisY({
            anchor: "left",
            label: "consumption (kWh)",
        }),
        Plot.axisY({
            anchor: "right",
            label: "rate / cost (p)",
            tickFormat: y => (y * scaleFactor).toFixed(0),
        }),
    ],
});

const chart = document.getElementById("chart");
chart.replaceChildren(plot);
