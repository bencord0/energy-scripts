.mode table

-- How much energy was consumed, per unit price?
SELECT
    r.value AS price,
    SUM(c.consumption) AS usage,
    SUM(r.value * c.consumption) AS cost
FROM consumption as c
LEFT JOIN tariff_rates as r
ON
    c.interval_start = r.valid_from
JOIN products AS p
ON
    r.product_code = p.product_code
    AND r.tariff_code = p.tariff_code
WHERE p.type = 'IMPORT'
GROUP BY price;
