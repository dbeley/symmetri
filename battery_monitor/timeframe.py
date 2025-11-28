from __future__ import annotations

import time
from dataclasses import dataclass
from typing import Optional

SECONDS_PER_HOUR = 3600
SECONDS_PER_DAY = 24 * SECONDS_PER_HOUR
APPROX_DAYS_PER_MONTH = 30


@dataclass(frozen=True)
class Timeframe:
    label: str
    seconds: Optional[float]
    hours: int = 0
    days: int = 0
    months: int = 0

    def since_timestamp(self, *, now: Optional[float] = None) -> Optional[float]:
        if self.seconds is None:
            return None
        reference = now if now is not None else time.time()
        return reference - self.seconds


def _validate_non_negative(value: int, name: str) -> None:
    if value < 0:
        raise ValueError(f"{name} must be zero or greater")


def _plural_suffix(value: int, singular: str, plural: str) -> str:
    return singular if value == 1 else plural


def build_timeframe(
    *,
    hours: int = 6,
    days: int = 0,
    months: int = 0,
    all_time: bool = False,
) -> Timeframe:
    """Build a normalized timeframe with precedence months > days > hours."""
    _validate_non_negative(hours, "hours")
    _validate_non_negative(days, "days")
    _validate_non_negative(months, "months")

    if all_time:
        return Timeframe(label="all", seconds=None)

    if months:
        seconds = months * APPROX_DAYS_PER_MONTH * SECONDS_PER_DAY
        label = f"last_{months}_{_plural_suffix(months, 'month', 'months')}"
        return Timeframe(label=label, seconds=seconds, months=months)

    if days:
        seconds = days * SECONDS_PER_DAY
        label = f"last_{days}_{_plural_suffix(days, 'day', 'days')}"
        return Timeframe(label=label, seconds=seconds, days=days)

    if hours == 0:
        raise ValueError("hours must be at least 1 when days and months are zero")

    seconds = hours * SECONDS_PER_HOUR
    label = f"last_{hours}_{_plural_suffix(hours, 'hour', 'hours')}"
    return Timeframe(label=label, seconds=seconds, hours=hours)


def timeframe_seconds(timeframe: Timeframe) -> Optional[float]:
    return timeframe.seconds


def since_timestamp(
    timeframe: Timeframe, *, now: Optional[float] = None
) -> Optional[float]:
    return timeframe.since_timestamp(now=now)
