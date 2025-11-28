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
