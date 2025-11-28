def test_twenty_four_hours_bucket_hourly():
    from battery_monitor.cli import _bucket_span_seconds, _bucket_start
    from battery_monitor.timeframe import build_timeframe
    from datetime import datetime

    timeframe = build_timeframe(days=1)
    span = _bucket_span_seconds(timeframe)
    sample_dt = datetime.now().replace(minute=37, second=12, microsecond=123456)
    bucket = _bucket_start(sample_dt.timestamp(), span)

    assert span == 3600
    assert bucket.hour == sample_dt.hour  # keeps one bucket per hour
    assert (bucket.minute, bucket.second, bucket.microsecond) == (0, 0, 0)


def test_six_hours_bucket_every_twenty_minutes():
    from battery_monitor.cli import _bucket_span_seconds, _bucket_start
    from battery_monitor.timeframe import build_timeframe
    from datetime import datetime

    timeframe = build_timeframe(hours=6)
    span = _bucket_span_seconds(timeframe)
    sample_dt = datetime.now().replace(minute=37, second=42, microsecond=654321)
    bucket = _bucket_start(sample_dt.timestamp(), span)

    assert span == 20 * 60
    assert bucket.minute == 20 * (sample_dt.minute // 20)
    assert (bucket.second, bucket.microsecond) == (0, 0)


def test_default_graph_path_has_timeframe_and_timestamp(tmp_path):
    from datetime import datetime, timezone

    from battery_monitor.cli import _default_graph_path
    from battery_monitor.timeframe import build_timeframe

    now = datetime(2025, 11, 28, 1, 30, 42, tzinfo=timezone.utc)
    timeframe = build_timeframe(hours=3)
    path = _default_graph_path(timeframe.label, base_dir=tmp_path, now=now)

    assert path.parent == tmp_path
    assert path.name == "battery_monitor_last_3_hours_2025-11-28_01-30-42_UTC.png"


def test_average_discharge_and_runtime_estimates():
    from battery_monitor.cli import (
        _average_discharge_w,
        _estimate_runtime_hours,
        _format_runtime,
    )
    from battery_monitor.db import Sample

    def sample(
        ts: float,
        energy_now: float,
        *,
        energy_full: float | None = None,
        energy_full_design: float | None = None,
    ) -> Sample:
        return Sample(
            ts=ts,
            percentage=None,
            capacity_pct=None,
            health_pct=None,
            energy_now_wh=energy_now,
            energy_full_wh=energy_full,
            energy_full_design_wh=energy_full_design,
            status=None,
            source_path="/dev/null",
        )

    samples = [
        sample(0, 60.0, energy_full=60.0, energy_full_design=70.0),
        sample(
            300, 59.6, energy_full=60.0, energy_full_design=70.0
        ),  # 0.4 Wh over 5m -> 4.8 W
        sample(600, 59.2, energy_full=60.0, energy_full_design=70.0),
    ]

    avg = _average_discharge_w(samples)
    runtime_hours = _estimate_runtime_hours(avg, current_sample=samples[-1])

    assert avg is not None
    assert round(avg, 2) == 4.8
    assert runtime_hours is not None
    assert round(runtime_hours, 2) == 12.5
    assert _format_runtime(runtime_hours) == "12h30m"

    # Falls back to design capacity when reported full charge is missing
    design_sample = sample(3600, 55.0, energy_full=None, energy_full_design=80.0)
    design_runtime = _estimate_runtime_hours(avg, current_sample=design_sample)
    assert design_runtime is not None
    assert round(design_runtime, 2) == 16.67


def test_average_discharge_ignores_large_gaps():
    from battery_monitor.cli import _average_discharge_w, _estimate_runtime_hours
    from battery_monitor.db import Sample

    def sample(ts: float, energy_now: float) -> Sample:
        return Sample(
            ts=ts,
            percentage=None,
            capacity_pct=None,
            health_pct=None,
            energy_now_wh=energy_now,
            energy_full_wh=60.0,
            energy_full_design_wh=70.0,
            status=None,
            source_path="/dev/null",
        )

    samples = [
        sample(0, 60.0),
        sample(300, 59.5),  # 0.5 Wh drop over 5m -> 6 W
        sample(1800, 59.4),  # 25m gap should be ignored with the tighter window
    ]

    avg = _average_discharge_w(samples)

    assert avg is not None
    assert round(avg, 2) == 6.0

    runtime_hours = _estimate_runtime_hours(avg, current_sample=samples[-1])
    assert runtime_hours is not None
    assert round(runtime_hours, 2) == 10.0


def test_average_discharge_ignores_charging():
    from battery_monitor.cli import _average_discharge_w
    from battery_monitor.db import Sample

    def sample(ts: float, energy_now: float, status: str) -> Sample:
        return Sample(
            ts=ts,
            percentage=None,
            capacity_pct=None,
            health_pct=None,
            energy_now_wh=energy_now,
            energy_full_wh=60.0,
            energy_full_design_wh=70.0,
            status=status,
            source_path="/dev/null",
        )

    samples = [
        sample(0, 60.0, "Discharging"),
        sample(300, 59.0, "Discharging"),  # 1 Wh drop over 5m -> 12 W
        sample(600, 60.0, "Charging"),  # Charging should not counteract discharge
        sample(900, 59.5, "Discharging"),  # transition back to battery use
        sample(1200, 59.0, "Discharging"),
    ]

    avg = _average_discharge_w(samples)

    assert avg is not None
    assert round(avg, 2) == 9.0


def test_average_charge_rate_tracks_charging_only():
    from battery_monitor.cli import _average_charge_w
    from battery_monitor.db import Sample

    def sample(ts: float, energy_now: float, status: str) -> Sample:
        return Sample(
            ts=ts,
            percentage=None,
            capacity_pct=None,
            health_pct=None,
            energy_now_wh=energy_now,
            energy_full_wh=60.0,
            energy_full_design_wh=70.0,
            status=status,
            source_path="/dev/null",
        )

    samples = [
        sample(0, 50.0, "Charging"),
        sample(300, 52.0, "Charging"),  # +2 Wh over 5m -> 24 W
        sample(600, 52.5, "Charging"),  # +0.5 Wh over 5m -> 6 W
        sample(900, 52.2, "Discharging"),  # discharge segment should be ignored
        sample(1200, 53.0, "Charging"),  # reset charging window
        sample(1500, 54.5, "Charging"),  # +1.5 Wh over 5m -> 18 W
    ]

    avg = _average_charge_w(samples)

    assert avg is not None
    assert round(avg, 2) == 16.0
