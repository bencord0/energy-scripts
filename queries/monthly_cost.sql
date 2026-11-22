.mode table

-- How much did I spend on energy each month?

WITH daily AS (
    SELECT
        SUBSTR(c.interval_start, 1, 10) as day,
        SUM(c.consumption * r.value) as cost,
        AVG(r.daily_standing_charge) as standing_charge
    FROM consumption AS c
    JOIN billed_tariff_rates AS r
    ON
        c.interval_start = r.valid_from
    WHERE r.type = 'IMPORT'
    GROUP BY day
),

costs AS (
    SELECT
        SUBSTR(day, 1, 7) as month,
        SUM(cost) as cost,
        SUM(standing_charge) as standing_charge
    FROM daily
    GROUP BY month
)

SELECT month, (cost + standing_charge) / 100 as pounds
FROM costs;
