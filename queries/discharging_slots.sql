.read queries/charging_slots.sql

.param set $EXPORT_TARIFF 'AGILE-OUTGOING-19-05-13'
-- The current price of octopus outgoing fixed
-- Set to 0 to disregard this filter
-- A higher number reduces the number of discharge slots selected
-- but can be used to game a vanity metric to demonstrate that agile outgoing
-- offers better prices than the fixed variant.
.param set $FIXED_EXPORT_BARRIER 12

CREATE TEMPORARY TABLE discharging_slots AS
WITH export_slots AS (
    SELECT
        r.valid_from,
        r.valid_to,
        r.value
    FROM tariff_rates AS r
    WHERE
        r.product_code = $EXPORT_TARIFF
        AND r.valid_from > $FROM_DATE
        AND r.value > (SELECT COALESCE(MAX(value), 0) FROM charging_slots)
        AND r.value > $FIXED_EXPORT_BARRIER
        AND r.valid_from NOT IN (SELECT valid_from FROM import_slots)
        AND r.valid_from > (SELECT MAX(valid_to) FROM charging_slots)
    ORDER BY r.value DESC
    LIMIT 4
),

-- Find gaps between the slots, and mark the discontinuities
marked_groups AS (
    SELECT
        valid_from,
        valid_to,
        value,
        -- Check if the current slot starts exactly when the previous slot ended.
        -- LAG(valid_to) peeks at the previous row's end time (ordered by time).
        CASE
            WHEN valid_from = LAG(valid_to) OVER (ORDER BY valid_from) THEN 0 -- Contiguous: belongs to same group
            ELSE 1 -- Gap found: starts a new group
        END as new_group
    FROM export_slots
),
grouped_slots AS (
    SELECT
        valid_from,
        valid_to,
        value,
        -- Calculate a running total of the 'new_group' flags.
        -- This assigns a unique ID to each contiguous block of time slots.
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
    printf('%0.2f', value) as value,
    printf('%0.2f', value - (SELECT MAX(value) FROM charging_slots)) as profit
FROM candidate_slots
ORDER BY value DESC;

.mode ascii
SELECT 'Outgoing Agile Discharging Slots
';

.mode table
SELECT
    SUBSTR(valid_from, 0, 17) AS start,
    SUBSTR(valid_to, 0, 17)   AS end,
    duration_hours            AS duration,
    value                     AS avg,
    profit                    AS profit
FROM discharging_slots;
