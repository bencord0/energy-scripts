-- How much energy was consumed, per unit price?

SELECT
    r.value,
    SUM(c.consumption),
    (r.value * SUM(c.consumption))
FROM
    consumption as c
LEFT JOIN
    tariff_rates as r
ON
    c.interval_start = r.valid_from
GROUP BY
    r.value
