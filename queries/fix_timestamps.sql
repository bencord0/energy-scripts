-- Update consumption table
UPDATE consumption SET
    interval_start = strftime('%Y-%m-%dT%H:%M:00Z', interval_start),
    interval_end = strftime('%Y-%m-%dT%H:%M:00Z', interval_end);

-- Update generation table (if it uses the same structure)
UPDATE consumption SET
    interval_start = strftime('%Y-%m-%dT%H:%M:00Z', interval_start),
    interval_end = strftime('%Y-%m-%dT%H:%M:00Z', interval_end);

-- Update tariff_rates table
UPDATE tariff_rates SET
    valid_from = strftime('%Y-%m-%dT%H:%M:00Z', valid_from),
    valid_to = strftime('%Y-%m-%dT%H:%M:00Z', valid_to);
