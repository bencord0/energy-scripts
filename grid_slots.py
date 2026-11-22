"""Provisional grid import/export per half-hour from GivEnergy inverters.

Derived from the raw lifetime counters in `battery_grid_totals`. A slot's
boundary reading is an inverter's first sample at or after the boundary, so
slots spanning two day files are covered. Only inverters that sampled during
the slot count, so an inverter isn't read as idle before it was installed.

Every inverter reads the same grid meter, but joined at different times, so
their lifetime totals differ while their per-slot deltas should agree. The
median of the valid deltas is written to `consumption` until Octopus meter
data replaces it; Octopus rows (or rows of unknown provenance) are never
overwritten.
"""

from datetime import datetime, timedelta
from statistics import median

# A 100 A single-phase supply delivers ~11.5 kWh per half-hour; anything
# beyond twice that is a counter glitch, not usage.
GRID_MAX_KWH_PER_SLOT = 25.0


def counter_delta(prev_total, curr_total):
    """Delta of a lifetime counter over one slot, or None for a reset/glitch.

    Unlike the daily totals (see `save_battery_data.detect_reset`), a
    lifetime counter never legitimately restarts at 0, so a drop is bad data.
    """
    delta = curr_total - prev_total
    if delta < 0 or delta > GRID_MAX_KWH_PER_SLOT:
        return None
    return delta


def slot_estimate(deltas):
    """Median of the valid per-inverter deltas, or None if there are none."""
    valid = [d for d in deltas if d is not None]
    if not valid:
        return None
    return median(valid)


def save_consumption_estimates(connection, account, starts):
    """Write the cross-inverter estimate for each slot in `starts`."""
    for start in sorted(set(starts)):
        end = slot_end(start)
        rows = inverter_slot_totals(connection, start, end)
        if not rows:
            continue

        grid_import = slot_estimate([counter_delta(r[1], r[3]) for r in rows])
        grid_export = slot_estimate([counter_delta(r[2], r[4]) for r in rows])
        if grid_import is None and grid_export is None:
            print(f"SKIP grid estimate at {start}: no valid inverter deltas")
            continue

        print(f"UPSERT grid estimate at {start}: import={grid_import} export={grid_export}")
        save_consumption_estimate(connection, account, start, end, grid_import, grid_export)


def slot_end(start):
    end = datetime.strptime(start, "%Y-%m-%dT%H:%M:%SZ") + timedelta(minutes=30)
    return end.strftime("%Y-%m-%dT%H:%M:%SZ")


def inverter_slot_totals(connection, start, end):
    """Per inverter: (inverter, start_import, start_export, end_import, end_export).

    Kept in step with queries/grid_slot_deltas.sql.
    """
    return connection.execute(
        """WITH inverters AS (
               SELECT DISTINCT inverter
               FROM battery_grid_totals
               WHERE timestamp >= :start
                 AND timestamp < :end
           ),
           boundaries AS (
               SELECT
                   inverter,
                   (SELECT b.timestamp
                    FROM battery_grid_totals AS b
                    WHERE b.inverter = inverters.inverter
                      AND b.timestamp >= :start
                    ORDER BY b.timestamp
                    LIMIT 1) AS start_at,
                   (SELECT b.timestamp
                    FROM battery_grid_totals AS b
                    WHERE b.inverter = inverters.inverter
                      AND b.timestamp >= :end
                    ORDER BY b.timestamp
                    LIMIT 1) AS end_at
               FROM inverters
           )
           SELECT
               boundaries.inverter,
               s.import AS start_import,
               s.export AS start_export,
               e.import AS end_import,
               e.export AS end_export
           FROM boundaries
           JOIN battery_grid_totals AS s
             ON s.inverter = boundaries.inverter
            AND s.timestamp = boundaries.start_at
           JOIN battery_grid_totals AS e
             ON e.inverter = boundaries.inverter
            AND e.timestamp = boundaries.end_at
           ORDER BY boundaries.inverter
        """,
        {"start": start, "end": end},
    ).fetchall()


def save_consumption_estimate(connection, account, start, end, grid_import, grid_export):
    # Each column only takes the estimate while its source is 'givenergy';
    # NULL source predates provenance tracking and is treated as Octopus.
    connection.execute(
        """INSERT INTO consumption(
               account,
               interval_start,
               interval_end,
               consumption,
               generation,
               consumption_source,
               generation_source)
           VALUES(:account, :start, :end, :import, :export,
                  CASE WHEN :import IS NULL THEN NULL ELSE 'givenergy' END,
                  CASE WHEN :export IS NULL THEN NULL ELSE 'givenergy' END)
           ON CONFLICT(account, interval_start) DO UPDATE SET
               consumption = CASE
                   WHEN consumption.consumption IS NULL
                     OR consumption.consumption_source IS 'givenergy'
                   THEN COALESCE(excluded.consumption, consumption.consumption)
                   ELSE consumption.consumption END,
               consumption_source = CASE
                   WHEN consumption.consumption IS NULL
                     OR consumption.consumption_source IS 'givenergy'
                   THEN COALESCE(excluded.consumption_source, consumption.consumption_source)
                   ELSE consumption.consumption_source END,
               generation = CASE
                   WHEN consumption.generation IS NULL
                     OR consumption.generation_source IS 'givenergy'
                   THEN COALESCE(excluded.generation, consumption.generation)
                   ELSE consumption.generation END,
               generation_source = CASE
                   WHEN consumption.generation IS NULL
                     OR consumption.generation_source IS 'givenergy'
                   THEN COALESCE(excluded.generation_source, consumption.generation_source)
                   ELSE consumption.generation_source END
        """,
        {
            "account": account,
            "start": start,
            "end": end,
            "import": grid_import,
            "export": grid_export,
        },
    )
