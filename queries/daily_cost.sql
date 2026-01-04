.mode table
.param set $STANDING_CHARGE 48.79
.param set $HISTORY 50

-- How much did I spend on energy each day?
-- Sum the consumption * price rate for all slots, midnight to midnight (UTC).
WITH costs AS (
    SELECT
        SUBSTR(c.interval_start, 1, 10) as day,
        SUM(c.consumption * r.value) AS cost
    FROM consumption AS c
    LEFT JOIN tariff_rates AS r
    ON
        c.interval_start = r.valid_from
    GROUP BY day
    ORDER BY day DESC
    LIMIT $HISTORY
)

SELECT day, (cost + $STANDING_CHARGE) / 100 as pounds
FROM costs
ORDER BY day ASC;
