from __future__ import annotations


def _downsample(values: list[float], target: int) -> list[float]:
    if len(values) <= target:
        return values
    step = len(values) / target
    return [values[int(i * step)] for i in range(target)]


def _sparkline(values: list[float]) -> str:
    chars = ".:-=+*#%@"
    min_v = min(values)
    max_v = max(values)
    span = max_v - min_v

    if span < 1e-9:
        line = "=" * len(values)
    else:
        scale = len(chars) - 1

        def to_char(val: float) -> str:
            idx = int((val - min_v) / span * scale)
            return chars[min(idx, scale)]

        line = "".join(to_char(v) for v in values)

    return f"{min_v:.0f}% {line} {max_v:.0f}%"


def _bar_graph(values: list[float], *, height: int, target_width: int) -> str:
    """Render a multi-line ASCII bar graph scaled to 0-100%."""
    if not values:
        return ""

    height = max(4, height)
    rows = max(4, height - 1)
    rows = int(round(rows / 4.0) * 4)  # keep ticks evenly spaced every 25%
    clamped = [max(0.0, min(100.0, v)) for v in values]
    downsampled = _downsample(clamped, target=target_width)

    normalized = [int(round(v / 100 * rows)) for v in downsampled]

    tick_rows: dict[int, int] = {}
    for pct in (100, 75, 50, 25, 0):
        row = int(round(pct / 100 * rows))
        tick_rows[row] = pct

    lines: list[str] = []
    for row in range(rows, -1, -1):
        label = tick_rows.get(row)
        axis = f"{label:>3}%" if label is not None else "    "
        bars = []
        for level in normalized:
            if level >= row:
                bars.append("#")
            elif label is not None:
                bars.append("-")
            else:
                bars.append(" ")
        lines.append(f"{axis} | {''.join(bars)}")

    min_v = min(downsampled)
    avg_v = sum(downsampled) / len(downsampled)
    max_v = max(downsampled)
    lines.append(f"{'':4} +{'-' * (len(downsampled) + 1)}")
    lines.append(f"{'':4} min {min_v:>5.1f}%  avg {avg_v:>5.1f}%  max {max_v:>5.1f}%")

    return "\n".join(lines)
