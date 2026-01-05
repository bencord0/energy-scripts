.read queries/charging_slots.sql

.param set $EXPORT_PRICE 15.0

CREATE TEMPORARY TABLE discharging_slots AS
WITH export_slots AS (
    SELECT
        r.valid_from,
        r.valid_to,
        $EXPORT_PRICE as value
    FROM tariff_rates AS r
    JOIN products AS p ON r.product_code = p.product_code AND r.tariff_code = p.tariff_code
    WHERE
        p.type = 'EXPORT'
        AND r.valid_from > $FROM_DATE
        -- Filter for slots where the fixed export price is better than the charging cost
        AND $EXPORT_PRICE > (SELECT COALESCE(MAX(value), 0) FROM charging_slots)
        AND r.valid_from NOT IN (SELECT valid_from FROM import_slots)
    GROUP BY r.valid_from -- Ensure we only have one row per 30m slot if multiple export tariffs exist
),

-- Find gaps between the slots, and mark the discontinuities
marked_groups AS (
    SELECT
        valid_from,
        valid_to,
        value,
        CASE
            WHEN valid_from = LAG(valid_to) OVER (ORDER BY valid_from) THEN 0
            ELSE 1
        END as new_group
    FROM export_slots
),
grouped_slots AS (
    SELECT
        valid_from,
        valid_to,
        value,
        SUM(new_group) OVER (ORDER BY valid_from) AS group_id
    FROM marked_groups
),
candidate_slots AS (
    SELECT
        MIN(valid_from) as valid_from,
        MAX(valid_to) as valid_to,
        (strftime('%s', MAX(valid_to)) - strftime('%s', MIN(valid_from))) / 3600.0 as duration_hours,
        AVG(value) as value
    FROM grouped_slots
    GROUP BY group_id
)
SELECT
    valid_from,
    valid_to,
    duration_hours,
    value,
    value - (SELECT MAX(value) FROM charging_slots) as profit
FROM candidate_slots
ORDER BY duration_hours DESC;

.mode ascii
SELECT 'Outgoing Octopus Discharging Slots
';

.mode table
SELECT * from discharging_slots;
