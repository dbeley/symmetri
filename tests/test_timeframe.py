import pytest

from battery_monitor.timeframe import since_timestamp, timeframe_seconds


def test_timeframe_seconds_supports_last_hour():
    assert timeframe_seconds("last_1h") == 3600
    assert timeframe_seconds("last-1h") == 3600


def test_timeframe_seconds_supports_long_ranges():
    day_seconds = 86400
    assert timeframe_seconds("last_year") == 365 * day_seconds
    assert timeframe_seconds("all") is None


def test_since_timestamp_uses_reference_now():
    now = 1_700_000_000.0
    assert since_timestamp("last_1h", now=now) == now - 3600


def test_since_timestamp_allows_unbounded():
    assert since_timestamp("all") is None


def test_timeframe_seconds_rejects_removed_alias():
    with pytest.raises(ValueError):
        timeframe_seconds("max")

    with pytest.raises(ValueError):
        since_timestamp("max")
