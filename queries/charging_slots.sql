-- Find the next cheapest time slots between the agile peak periods
-- order by value, and select the cheapest of the day.

-- Parameters

-- Granularity is one of the few tunables we have
-- 1.0 does not remove any slots, so don't go below that.
-- 1.5 gives us longer candidates
-- 2.0 to 4.0 gives us cheaper candidates, but smaller time windows
.param set $GRANULARITY 8.0

-- Only inspect future time slots
.param set $FROM_DATE strftime('%Y-%m-%dT%H:%M')

CREATE TEMPORARY TABLE import_slots AS
WITH raw_slots AS (
    SELECT
        r.valid_from,
        r.valid_to,
        r.value
    FROM tariff_rates AS r
    JOIN products AS p
    ON
        r.product_code = p.product_code
        AND r.tariff_code = p.tariff_code
    WHERE
        r.valid_from > $FROM_DATE
        AND p.type = 'IMPORT'
)
SELECT * FROM raw_slots
    ORDER BY value ASC
    -- Draw a cutoff and filter out expensive slots after N rows.
    LIMIT (SELECT FLOOR(COUNT(*) / $GRANULARITY) FROM raw_slots);

-- Always include cheap / negative slots
INSERT INTO import_slots(valid_from, valid_to, value)
    SELECT
        r.valid_from,
        r.valid_to,
        r.value
    FROM tariff_rates AS r
    JOIN products AS p
    ON
        r.product_code = p.product_code
        AND r.tariff_code = p.tariff_code
    WHERE
        r.valid_from > $FROM_DATE
        AND r.value <= 0
        AND p.type = 'IMPORT';

-- Re-order back in chronological order
-- Deduplicate as needed
CREATE TEMPORARY TABLE charging_slots AS
WITH ordered_slots AS (
    SELECT DISTINCT valid_from, valid_to, value
    FROM import_slots
    ORDER BY valid_from
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
    FROM ordered_slots
),
grouped_slots AS (
    SELECT
        valid_from,
        valid_to,
        value,
        -- Calculate a running total of the 'new_group' flags.
        -- This assigns a unique ID to each contiguous block of time slots.
        SUM(new_group) OVER (ORDER BY valid_from) as group_id
    FROM marked_groups
),
candidate_slots AS (
    SELECT
        MIN(valid_from) as valid_from,
        MAX(valid_to) as valid_to,
        AVG(value) as value,
        MIN(value) as min_value,
        (strftime('%s', MAX(valid_to)) - strftime('%s', MIN(valid_from))) / 3600.0 as duration_hours
    FROM grouped_slots
    GROUP BY group_id
    ORDER BY valid_from
)
SELECT
    valid_from,
    valid_to,
    duration_hours,
    printf('%.2f', value) as value,
    printf('%.2f', min_value) as min_value
FROM candidate_slots
-- prioritise longer, cheaper slots
-- but don't mess up if the value is negative.
-- for a 1 hour slot priced at -2p is better than if it is +1p.
-- and don't explode if it costs 0p
-- -2 -> -1/2        -> 1 / 98   <- this is clearly the best number
-- +0 -> +1/0 (oops) -> 1 / 100  <- this doesn't explode
-- +1 -> +1/1        -> 1 / 101  <- this is fine, but not best
-- +2 -> +1/2        -> 1 / 102  <- easily a poorer number
--
-- a 2 hour timeslot at 1p, is better than a 1 hr at 2p
-- +2 / (100 + 1) = 0.0198
-- +1 / (100 + 2) = 0.0098
ORDER BY (duration_hours / (100 + value)) DESC;

.mode ascii
SELECT 'Agile Octopus Charging Slots
';

.mode table
SELECT
    SUBSTR(valid_from, 0, 17) AS start,
    SUBSTR(valid_to, 0, 17)   AS end,
    duration_hours            AS duration,
    value                     AS avg,
    min_value                 AS min
FROM charging_slots
    -- Comment this to order by value
    -- Uncomment to order by time
    ORDER BY valid_from
;
