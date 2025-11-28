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


def test_event_helpers_group_by_timestamp(tmp_path: Path):
    db_path = tmp_path / "battery.db"
    samples = [
        db.Sample(
            ts=1.0,
            percentage=50.0,
            capacity_pct=90.0,
            health_pct=95.0,
            energy_now_wh=40.0,
            energy_full_wh=80.0,
            energy_full_design_wh=90.0,
            status="Discharging",
            source_path="BAT0",
        ),
        db.Sample(
            ts=1.0,
            percentage=60.0,
            capacity_pct=91.0,
            health_pct=96.0,
            energy_now_wh=20.0,
            energy_full_wh=40.0,
            energy_full_design_wh=50.0,
            status="Charging",
            source_path="BAT1",
        ),
        db.Sample(
            ts=5.0,
            percentage=75.0,
            capacity_pct=89.0,
            health_pct=94.0,
            energy_now_wh=50.0,
            energy_full_wh=70.0,
            energy_full_design_wh=80.0,
            status="Discharging",
            source_path="BAT0",
        ),
    ]

    db.init_db(db_path)
    db.insert_samples(db_path, samples)

    assert db.count_events(db_path) == 2
    assert db.count_events(db_path, since_ts=2.0) == 1

    first_event = db.fetch_first_event(db_path)
    latest_event = db.fetch_latest_event(db_path)
    recent_events = db.fetch_recent_events(db_path)

    assert len(first_event) == 2
    assert {sample.source_path for sample in first_event} == {"BAT0", "BAT1"}
    assert len(latest_event) == 1
    assert latest_event[0].ts == 5.0
    assert len(recent_events) == 2  # both timestamps represented
