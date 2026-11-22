.mode table
.param set $HISTORY 50

-- How much did I spend on energy each day?
-- Sum the consumption * price rate for all slots, midnight to midnight (UTC),
-- on whichever tariff the import agreement covered.
WITH costs AS (
    SELECT
        SUBSTR(c.interval_start, 1, 10) as day,
        SUM(c.consumption * r.value) AS cost,
        AVG(r.daily_standing_charge) AS standing_charge
    FROM consumption AS c
    JOIN billed_tariff_rates AS r
    ON
        c.interval_start = r.valid_from
    WHERE r.type = 'IMPORT'
    GROUP BY day
    ORDER BY day DESC
    LIMIT $HISTORY
)

SELECT day, (cost + standing_charge) / 100 as pounds
FROM costs
ORDER BY day ASC;
