.mode json
-- Find the next cheapest time slots between the agile peak periods
-- order by value, and select the cheapest of the day.
WITH raw_slots AS (
    SELECT valid_from, valid_to, value
    FROM tariff_rates
    WHERE
        valid_from > DATE()
),
cheap_slots AS (
    SELECT * FROM raw_slots
    ORDER BY value ASC
    -- This is one of the few tunables we have
    -- 1.0 does not remove any slots, so don't go below that.
    -- 1.5 gives us longer candidates
    -- 2.0 to 4.0 gives us cheaper candidates, but smaller time windows
    LIMIT (SELECT FLOOR(COUNT(*) / 1.8) FROM raw_slots)
),
-- Re-order back in chronological order
ordered_slots AS (
    SELECT valid_from, valid_to, value
    FROM cheap_slots
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
        (strftime('%s', MAX(valid_to)) - strftime('%s', MIN(valid_from))) / 3600.0 as duration_hours
    FROM grouped_slots
    GROUP BY group_id
    ORDER BY valid_from
)
SELECT
    valid_from,
    valid_to,
    duration_hours,
    value
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
