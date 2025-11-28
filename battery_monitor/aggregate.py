from __future__ import annotations

from collections import defaultdict
from pathlib import Path
from typing import Iterable

from .db import Sample


def _sum_or_none(values: Iterable[float | None]) -> float | None:
    present = [value for value in values if value is not None]
    return sum(present) if present else None


def _avg_or_none(values: Iterable[float | None]) -> float | None:
    present = [value for value in values if value is not None]
    return sum(present) / len(present) if present else None


def _percent(numerator: float | None, denominator: float | None) -> float | None:
    if numerator is None or denominator in (None, 0):
        return None
    try:
        return (numerator / denominator) * 100.0
    except ZeroDivisionError:
        return None


def aggregate_group(samples: Iterable[Sample]) -> Sample:
    group = list(samples)
    if not group:
        raise ValueError("Cannot aggregate an empty sample group")
    ts = group[0].ts

    energy_now_wh = _sum_or_none(sample.energy_now_wh for sample in group)
    energy_full_wh = _sum_or_none(sample.energy_full_wh for sample in group)
    energy_full_design_wh = _sum_or_none(
        sample.energy_full_design_wh for sample in group
    )
    capacity_pct = _avg_or_none(sample.capacity_pct for sample in group)

    percentage = _percent(energy_now_wh, energy_full_wh)
    if percentage is None:
        percentage = _avg_or_none(sample.percentage for sample in group)

    health_pct = _percent(energy_full_wh, energy_full_design_wh)
    if health_pct is None:
        health_pct = _avg_or_none(sample.health_pct for sample in group)

    status_options = sorted({sample.status for sample in group if sample.status})
    status = None
    if status_options:
        status = status_options[0] if len(status_options) == 1 else "mixed"

    source_path = "+".join(sorted({Path(sample.source_path).name for sample in group}))

    return Sample(
        ts=ts,
        percentage=percentage,
        capacity_pct=capacity_pct,
        health_pct=health_pct,
        energy_now_wh=energy_now_wh,
        energy_full_wh=energy_full_wh,
        energy_full_design_wh=energy_full_design_wh,
        status=status,
        source_path=source_path,
    )


def aggregate_samples_by_timestamp(samples: Iterable[Sample]) -> list[Sample]:
    buckets: dict[float, list[Sample]] = defaultdict(list)
    for sample in samples:
        buckets[sample.ts].append(sample)
    aggregated = [
        aggregate_group(buckets[bucket_ts]) for bucket_ts in sorted(buckets.keys())
    ]
    return aggregated
