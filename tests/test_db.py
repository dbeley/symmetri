import time
from pathlib import Path

from battery_monitor import db
from battery_monitor.sysfs import BatteryReading


def test_db_roundtrip(tmp_path: Path):
    db_path = tmp_path / "battery.db"
    reading = BatteryReading(
        path=tmp_path / "BAT0",
        capacity_pct=90,
        percentage=75.0,
        energy_now_wh=50.0,
        energy_full_wh=70.0,
        energy_full_design_wh=80.0,
        health_pct=87.5,
        status="Discharging",
    )
    ts = time.time()
    sample = db.create_sample_from_reading(reading, ts=ts)

    db.init_db(db_path)
    db.insert_sample(db_path, sample)

    rows = list(db.fetch_samples(db_path))
    assert len(rows) == 1
    stored = rows[0]
    assert stored.ts == ts
    assert stored.percentage == 75.0
    assert stored.health_pct == 87.5
    assert stored.status == "Discharging"


def test_insert_samples_bulk(tmp_path: Path):
    db_path = tmp_path / "battery.db"
    readings = [
        BatteryReading(
            path=tmp_path / f"BAT{i}",
            capacity_pct=90 + i,
            percentage=70.0 + i,
            energy_now_wh=40.0 + i,
            energy_full_wh=60.0 + i,
            energy_full_design_wh=80.0 + i,
            health_pct=85.0 + i,
            status="Charging",
        )
        for i in range(2)
    ]
    ts = time.time()
    samples = [db.create_sample_from_reading(reading, ts=ts) for reading in readings]

    db.init_db(db_path)
    db.insert_samples(db_path, samples)

    rows = list(db.fetch_samples(db_path))
    assert len(rows) == 2
    assert [row.source_path for row in rows] == [str(r.path) for r in readings]
