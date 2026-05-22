export function getTimeWindow(startStr, endStr) {
    const startDate = new Date(startStr);
    const endDate = new Date(endStr);
    const durationHours = (endDate - startDate) / (1000 * 60 * 60);

    if (durationHours > 24 * 20) {
        return '1d';
    } else if (durationHours > 24 * 8) {
        return '1h';
    } else {
        return '30m';
    }
}

export async function getConsumption(startStr, endStr) {
    const timeWindow = getTimeWindow(startStr, endStr);

    const query = new URLSearchParams({
        "start": startStr,
        "end": endStr,
        "window": timeWindow,
    });

    let response = await fetch("/api/consumption?" + query.toString());
    let data = await response.json();
    return { data, timeWindow };
}

export async function getPriceDistribution(startStr, endStr) {
    const query = new URLSearchParams({
        "start": startStr,
        "end": endStr,
    });

    let response = await fetch("/api/price-distribution?" + query.toString());
    let data = await response.json();

    const timeWindow = getTimeWindow(startStr, endStr);
    return { data, timeWindow };
}

export async function getConsumptionByTimeOfDay(startStr, endStr) {
    const query = new URLSearchParams({
        "start": startStr,
        "end": endStr,
    });

    let response = await fetch("/api/consumption-by-time?" + query.toString());
    let data = await response.json();

    const timeWindow = getTimeWindow(startStr, endStr);
    return { data, timeWindow };
}

export async function getStandingCharge(startStr, endStr, type = 'IMPORT') {
    const query = new URLSearchParams({
        "start": startStr,
        "end": endStr,
        "type": type,
    });

    let response = await fetch("/api/standing-charge?" + query.toString());
    let data = await response.json();

    return data;
}

export async function getAgilePredictions(startStr, endStr, region) {
    const timeWindow = getTimeWindow(startStr, endStr);

    const query = new URLSearchParams({
        "start": startStr,
        "end": endStr,
        "region": region,
        "window": timeWindow,
    });

    let response = await fetch("/api/agile-prediction?" + query.toString());
    let data = await response.json();

    return data;
}

export async function getDataLimits() {
    let response = await fetch("/api/data-limits");
    let data = await response.json();

    return data;
}
