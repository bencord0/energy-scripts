export function formatCost(pence) {
    if (pence >= 100) {
        return `£${(pence / 100).toFixed(2)}`;
    } else {
        return `${pence.toFixed(2)}p`;
    }
}

export function getTimeWindowInfo(timeWindow) {
    const slotsPerDay = {'1d': 1, '1h': 24, '30m': 48}[timeWindow]
    const intervalHours = 24 / slotsPerDay;
    return { slotsPerDay, intervalHours };
}
