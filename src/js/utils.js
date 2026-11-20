export function formatCost(pence) {
    const absPence = Math.abs(pence);
    const sign = pence < 0 ? "-" : "";
    if (absPence >= 100) {
        return `${sign}£${(absPence / 100).toFixed(2)}`;
    } else if (!(absPence % 1)) {
        return `${sign}${absPence.toFixed(0)}p`;
    } else {
        return `${sign}${absPence.toFixed(2)}p`;
    }
}

export function getTimeWindowInfo(timeWindow) {
    const intervalHours = { '1w': 168, '1d': 24, '1h': 1, '30m': 0.5 }[timeWindow];
    const slotsPerDay = 24 / intervalHours;
    return { slotsPerDay, intervalHours };
}

export function weekStartUTC(date) {
    const d = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()));
    const mondayOffset = (d.getUTCDay() + 6) % 7;
    d.setUTCDate(d.getUTCDate() - mondayOffset);
    return d;
}

// Replaces the tariff-only price fields from the API with the price after
// Saving Sessions (Happy Hours, Power Down/Up rewards).
const incentiveFields = [
    ['import_rate', 'import_incentive'],
    ['export_rate', 'export_incentive'],
    ['cost', 'cost_incentive'],
    ['sale', 'sale_incentive'],
];

export function applyIncentives(d) {
    const corrected = { ...d };
    for (const [field, incentive] of incentiveFields) {
        if (d[field] === undefined) continue;
        corrected[`tariff_${field}`] = d[field];
        corrected[field] = d[field] + (d[incentive] || 0);
    }
    return corrected;
}
