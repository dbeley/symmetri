from battery_monitor.sparkline import _bar_graph, _sparkline


def test_sparkline_has_no_blank_segments():
    values = [75.9, 73.4, 70.9, 68.7, 67.5, 66.3]
    parts = _sparkline(values).split()
    assert parts == ["66%", "@*=-:.", "76%"]


def test_sparkline_shows_flat_lines():
    parts = _sparkline([50.0, 50.0, 50.0]).split()
    assert parts == ["50%", "===", "50%"]


def test_bar_graph_has_axis_and_stats():
    graph = _bar_graph([0, 25, 50, 75, 100], height=6, target_width=5)
    lines = graph.splitlines()

    assert lines[0].startswith("100% |")
    assert any(line.startswith(" 50% |") for line in lines)
    assert lines[-2].strip().startswith("+")
    assert lines[-1].strip().startswith("min")
    assert "avg  50.0%" in lines[-1]


def test_last_day_buckets_are_hourly():
    from battery_monitor.cli import _bucket_start
    from datetime import datetime

    # Choose a local time with a non-zero minute to ensure rounding only clears minutes.
    sample_dt = datetime.now().replace(minute=37, second=12, microsecond=123456)
    bucket = _bucket_start(sample_dt.timestamp(), "last_day")

    assert bucket.hour == sample_dt.hour  # no 2-hour grouping
    assert (bucket.minute, bucket.second, bucket.microsecond) == (0, 0, 0)
