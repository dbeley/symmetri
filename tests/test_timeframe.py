import pytest

from battery_monitor.timeframe import (
    APPROX_DAYS_PER_MONTH,
    SECONDS_PER_DAY,
    SECONDS_PER_HOUR,
    build_timeframe,
    since_timestamp,
)


def test_default_timeframe_is_last_six_hours():
    timeframe = build_timeframe()

    assert timeframe.hours == 6
    assert timeframe.seconds == 6 * SECONDS_PER_HOUR
    assert timeframe.label == "last_6_hours"


def test_days_and_months_take_precedence_over_hours():
    timeframe_days = build_timeframe(hours=2, days=1)
    timeframe_months = build_timeframe(hours=2, days=2, months=1)

    assert timeframe_days.days == 1
    assert timeframe_days.seconds == SECONDS_PER_DAY
    assert timeframe_days.label == "last_1_day"

    assert timeframe_months.months == 1
    assert timeframe_months.seconds == APPROX_DAYS_PER_MONTH * SECONDS_PER_DAY
    assert timeframe_months.label == "last_1_month"


def test_since_timestamp_uses_reference_now():
    now = 1_700_000_000.0
    timeframe = build_timeframe(hours=1)

    assert since_timestamp(timeframe, now=now) == now - SECONDS_PER_HOUR


def test_since_timestamp_allows_unbounded():
    timeframe = build_timeframe(all_time=True)

    assert since_timestamp(timeframe) is None
    assert timeframe.label == "all"


def test_invalid_inputs_raise():
    with pytest.raises(ValueError):
        build_timeframe(hours=0)
    with pytest.raises(ValueError):
        build_timeframe(days=-1)
