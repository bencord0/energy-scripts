.mode table
.param set $STANDING_CHARGE 48.79

-- How much did I spend on energy each month?

WITH costs AS (
    SELECT
        SUBSTR(c.interval_start, 1, 7) as month,
        SUM(c.consumption * r.value) as cost,

        -- Count of days in a month
        COUNT(DISTINCT SUBSTR(c.interval_start, 1, 10)) * $STANDING_CHARGE as standing_charge
    FROM consumption AS c
    LEFT JOIN tariff_rates AS r
    ON
        c.interval_start = r.valid_from
    GROUP BY month
)

SELECT month, (cost + standing_charge) / 100 as pounds
FROM costs;
