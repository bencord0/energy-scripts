.read queries/charging_slots.sql

.param set $EXPORT_TARIFF 'AGILE-OUTGOING-19-05-13'

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
        AND r.valid_from NOT IN (SELECT valid_from FROM import_slots)
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
        (strftime('%s', MAX(valid_to)) - strftime('%s', MIN(valid_from))) / 3600.0 as duration,
        AVG(value) as value
    FROM grouped_slots
    GROUP BY group_id
)
SELECT
    valid_from,
    valid_to,
    duration,
    value,
    value - (SELECT MAX(value) FROM charging_slots) as profit
FROM candidate_slots
ORDER BY value DESC;

.mode ascii
SELECT 'Outgoing Agile Discharging Slots
';

.mode table
SELECT * from discharging_slots;
