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
    const slotsPerDay = { '1d': 1, '1h': 24, '30m': 48 }[timeWindow]
    const intervalHours = 24 / slotsPerDay;
    return { slotsPerDay, intervalHours };
}
