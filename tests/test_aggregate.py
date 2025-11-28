from battery_monitor.aggregate import aggregate_group, aggregate_samples_by_timestamp
from battery_monitor.db import Sample


def _sample(
    ts: float,
    energy_now: float,
    energy_full: float,
    energy_design: float,
    *,
    capacity: float | None = None,
    status: str = "Discharging",
    source: str,
) -> Sample:
    return Sample(
        ts=ts,
        percentage=None,
        capacity_pct=capacity,
        health_pct=None,
        energy_now_wh=energy_now,
        energy_full_wh=energy_full,
        energy_full_design_wh=energy_design,
        status=status,
        source_path=source,
    )


def test_aggregate_group_combines_values_and_statuses():
    samples = [
        _sample(
            1.0,
            energy_now=10.0,
            energy_full=20.0,
            energy_design=25.0,
            capacity=90.0,
            source="BAT0",
        ),
        _sample(
            1.0,
            energy_now=5.0,
            energy_full=10.0,
            energy_design=15.0,
            capacity=95.0,
            status="Charging",
            source="BAT1",
        ),
    ]

    combined = aggregate_group(samples)

    assert combined.energy_now_wh == 15.0
    assert combined.energy_full_wh == 30.0
    assert combined.energy_full_design_wh == 40.0
    assert round(combined.percentage or 0.0, 2) == 50.0
    assert round(combined.health_pct or 0.0, 2) == 75.0
    assert combined.capacity_pct == 92.5  # average over the two samples
    assert combined.status == "mixed"
    assert combined.source_path == "BAT0+BAT1"


def test_aggregate_samples_by_timestamp_groups_events():
    samples = [
        _sample(1.0, 1.0, 2.0, 3.0, source="BAT0"),
        _sample(2.0, 2.0, 4.0, 6.0, source="BAT0"),
        _sample(1.0, 0.5, 1.0, 1.5, source="BAT1"),
    ]

    aggregated = aggregate_samples_by_timestamp(samples)

    assert len(aggregated) == 2
    assert aggregated[0].ts == 1.0
    assert aggregated[0].energy_now_wh == 1.5
    assert aggregated[1].ts == 2.0
    assert aggregated[1].energy_now_wh == 2.0
