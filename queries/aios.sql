SELECT
    b.start,
    b.end,
    SUM(b.charge),
    SUM(b.discharge),
    MAX(c.consumption),
    MAX(c.generation)

FROM charge as b

LEFT JOIN consumption as c
    ON c.interval_start = b.start

WHERE
    start >= '2026-10-22T00:00Z'
AND   end <  '2026-10-22T06:00Z'
GROUP BY
    start, end
;



