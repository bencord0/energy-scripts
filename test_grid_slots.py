"""Run with: python -m unittest test_grid_slots"""
import sqlite3
import tempfile
import unittest
from pathlib import Path

from db_migrate import migrate_db
from grid_slots import (
    GRID_MAX_KWH_PER_SLOT,
    counter_delta,
    inverter_slot_totals,
    save_consumption_estimate,
    save_consumption_estimates,
    slot_estimate,
)

QUERY = Path(__file__).resolve().parent / "queries" / "grid_slot_deltas.sql"

START = "2026-11-22T12:00:00Z"
END = "2026-11-22T12:30:00Z"


class CounterDeltaTest(unittest.TestCase):
    def test_inverters_with_different_uptimes_agree(self):
        # Same meter, different lifetime totals (CH2546R105 joined earlier).
        self.assertAlmostEqual(counter_delta(1967.8, 1968.3), 0.5)
        self.assertAlmostEqual(counter_delta(516.7, 517.2), 0.5)

    def test_drop_is_a_glitch_not_a_reset(self):
        # CH2546R105 at 2026-10-09T19:00Z, and the flicker after it.
        self.assertIsNone(counter_delta(2510.1, 1428.6))
        self.assertIsNone(counter_delta(1431.0, 1428.6))

    def test_implausible_jump_is_discarded(self):
        self.assertIsNone(counter_delta(1428.6, 1428.6 + GRID_MAX_KWH_PER_SLOT + 0.1))
        self.assertEqual(counter_delta(100.0, 100.0), 0.0)


class SlotEstimateTest(unittest.TestCase):
    def test_median_ignores_glitched_inverters(self):
        self.assertAlmostEqual(slot_estimate([0.5, None, 0.6]), 0.55)
        self.assertAlmostEqual(slot_estimate([0.5, 0.5, 9.0]), 0.5)

    def test_no_valid_inverter(self):
        self.assertIsNone(slot_estimate([None, None]))
        self.assertIsNone(slot_estimate([]))


class ConsumptionEstimateTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.template = Path(cls.tmp.name) / "template.sqlite3"
        migrate_db(cls.template)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def setUp(self):
        self.connection = sqlite3.connect(":memory:")
        sqlite3.connect(self.template).backup(self.connection)

    def tearDown(self):
        self.connection.close()

    def row(self):
        return self.connection.execute(
            """SELECT consumption, generation, consumption_source, generation_source
               FROM consumption WHERE account = 'A' AND interval_start = ?""",
            (START,),
        ).fetchone()

    def insert_octopus(self, consumption=None, generation=None):
        self.connection.execute(
            """INSERT INTO consumption(account, interval_start, interval_end,
                   consumption, generation, consumption_source, generation_source)
               VALUES('A', ?, ?, ?, ?,
                   CASE WHEN ? IS NULL THEN NULL ELSE 'octopus' END,
                   CASE WHEN ? IS NULL THEN NULL ELSE 'octopus' END)""",
            (START, END, consumption, generation, consumption, generation),
        )

    def samples(self, inverter, *samples):
        self.connection.executemany(
            """INSERT INTO battery_grid_totals(inverter, timestamp, import, export)
               VALUES(?, ?, ?, ?)""",
            [(inverter, *sample) for sample in samples],
        )

    def test_median_across_inverters_is_written(self):
        # Different lifetime totals, sampled at different times.
        self.samples("CH2546R105",
                     ("2026-11-22T12:01:00Z", 1967.8, 10.0),
                     ("2026-11-22T12:31:00Z", 1968.3, 10.1))
        # The 2026-10-09 counter drop.
        self.samples("CG2422G278",
                     ("2026-11-22T12:02:00Z", 2510.1, 5.0),
                     ("2026-11-22T12:32:00Z", 1428.6, 5.1))
        self.samples("CG2340G324",
                     ("2026-11-22T12:00:00Z", 516.7, 3.0),
                     ("2026-11-22T12:30:00Z", 517.4, 3.1))
        save_consumption_estimates(self.connection, "A", [START, START])

        consumption, generation, csource, gsource = self.row()
        self.assertAlmostEqual(consumption, 0.6)
        self.assertAlmostEqual(generation, 0.1)
        self.assertEqual((csource, gsource), ("givenergy", "givenergy"))

    def test_slot_spanning_day_files(self):
        # 23:30 is closed by the next day's first sample.
        self.samples("CH2546R105",
                     ("2026-11-21T23:31:00Z", 100.0, 0.0),
                     ("2026-11-21T23:59:00Z", 100.4, 0.0),
                     ("2026-11-22T00:01:48Z", 100.5, 0.0))
        rows = inverter_slot_totals(self.connection, "2026-11-21T23:30:00Z", "2026-11-22T00:00:00Z")
        self.assertEqual(rows, [("CH2546R105", 100.0, 0.0, 100.5, 0.0)])

    def test_inverter_without_end_sample_is_skipped(self):
        self.samples("CH2546R105", ("2026-11-22T12:01:00Z", 100.0, 0.0))
        self.samples("CG2340G324",
                     ("2026-11-22T12:01:00Z", 50.0, 0.0),
                     ("2026-11-22T12:31:00Z", 50.5, 0.0))
        rows = inverter_slot_totals(self.connection, START, END)
        self.assertEqual([r[0] for r in rows], ["CG2340G324"])

    def test_inverter_not_yet_installed_is_skipped(self):
        # CG2340G324 first reported in October; it mustn't read as 0 kWh before.
        self.samples("CH2546R105",
                     ("2026-07-01T12:01:00Z", 100.0, 0.0),
                     ("2026-07-01T12:31:00Z", 100.5, 0.0))
        self.samples("CG2340G324", ("2026-10-21T17:32:08Z", 50.0, 0.0))
        rows = inverter_slot_totals(self.connection, "2026-07-01T12:00:00Z", "2026-07-01T12:30:00Z")
        self.assertEqual([r[0] for r in rows], ["CH2546R105"])

    def test_diagnostic_query_agrees(self):
        self.samples("CH2546R105",
                     ("2026-11-22T12:01:00Z", 100.0, 1.0),
                     ("2026-11-22T12:31:00Z", 100.5, 1.2))
        self.samples("CG2340G324",
                     ("2026-11-22T12:02:00Z", 50.0, 2.0),
                     ("2026-11-22T12:29:00Z", 50.4, 2.1),
                     ("2026-11-22T12:33:00Z", 50.6, 2.2))
        diagnostic = self.connection.execute(
            QUERY.read_text(), {"start": START, "end": END},
        ).fetchall()
        self.assertEqual(
            [row[:5] for row in diagnostic],
            inverter_slot_totals(self.connection, START, END),
        )

    def test_nothing_written_without_valid_inverters(self):
        self.samples("CH2546R105",
                     ("2026-11-22T12:01:00Z", 2510.1, 5.0),
                     ("2026-11-22T12:31:00Z", 1428.6, 4.0))
        save_consumption_estimates(self.connection, "A", [START])
        self.assertIsNone(self.row())

    def test_estimate_never_overwrites_octopus(self):
        self.insert_octopus(consumption=1.25, generation=0.0)
        save_consumption_estimate(self.connection, "A", START, END, 9.0, 9.0)
        self.assertEqual(self.row(), (1.25, 0.0, "octopus", "octopus"))

    def test_estimate_never_overwrites_untracked_rows(self):
        # Rows written before provenance tracking count as Octopus.
        self.connection.execute(
            """INSERT INTO consumption(account, interval_start, interval_end, consumption, generation)
               VALUES('A', ?, ?, 1.25, 0.0)""",
            (START, END),
        )
        save_consumption_estimate(self.connection, "A", START, END, 9.0, 9.0)
        self.assertEqual(self.row(), (1.25, 0.0, None, None))

    def test_import_and_export_are_reconciled_independently(self):
        # Octopus import has landed, export hasn't yet.
        self.insert_octopus(consumption=1.25)
        save_consumption_estimate(self.connection, "A", START, END, 9.0, 0.4)
        self.assertEqual(self.row(), (1.25, 0.4, "octopus", "givenergy"))

    def test_newer_estimate_replaces_older_estimate(self):
        save_consumption_estimate(self.connection, "A", START, END, 0.5, 0.1)
        save_consumption_estimate(self.connection, "A", START, END, 0.6, None)
        self.assertEqual(self.row(), (0.6, 0.1, "givenergy", "givenergy"))

    def test_octopus_replaces_estimate(self):
        # Mirrors save_consumption_data.rs's import upsert.
        save_consumption_estimate(self.connection, "A", START, END, 0.5, 0.1)
        self.connection.execute(
            """INSERT INTO consumption(account, interval_start, interval_end, consumption, consumption_source)
               VALUES('A', ?, ?, 1.25, 'octopus')
               ON CONFLICT(account, interval_start) DO UPDATE SET
                   consumption=excluded.consumption,
                   consumption_source=excluded.consumption_source""",
            (START, END),
        )
        self.assertEqual(self.row(), (1.25, 0.1, "octopus", "givenergy"))


if __name__ == "__main__":
    unittest.main()
