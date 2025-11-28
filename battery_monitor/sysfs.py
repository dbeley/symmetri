from __future__ import annotations

import logging
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional

log = logging.getLogger(__name__)


@dataclass
class BatteryReading:
    path: Path
    capacity_pct: Optional[float]
    percentage: Optional[float]
    energy_now_wh: Optional[float]
    energy_full_wh: Optional[float]
    energy_full_design_wh: Optional[float]
    health_pct: Optional[float]
    status: Optional[str]


def _read_float(path: Path) -> Optional[float]:
    try:
        raw = path.read_text().strip()
    except FileNotFoundError:
        return None
    if not raw:
        return None
    try:
        return float(raw)
    except ValueError:
        log.debug("Non-numeric value in %s: %s", path, raw)
        return None


def _read_str(path: Path) -> Optional[str]:
    try:
        raw = path.read_text().strip()
    except FileNotFoundError:
        return None
    return raw or None


def _wh_from_energy(raw_value: Optional[float]) -> Optional[float]:
    if raw_value is None:
        return None
    # Sysfs energy values are reported in microwatt-hours
    return raw_value / 1_000_000.0


def _energy_wh_from_charge(
    charge_uah: Optional[float], voltage_uv: Optional[float]
) -> Optional[float]:
    if charge_uah is None or voltage_uv is None:
        return None
    # charge is in microamp-hours, voltage is in microvolts
    return (charge_uah * voltage_uv) / 1_000_000_000_000.0


def _read_voltage(path: Path) -> Optional[float]:
    for name in ("voltage_now", "voltage_min_design", "voltage_max_design"):
        value = _read_float(path / name)
        if value is not None:
            return value
    return None


def find_battery_paths(
    sysfs_root: Path = Path("/sys/class/power_supply"),
) -> Iterable[Path]:
    for candidate in sysfs_root.iterdir():
        if candidate.name.startswith("BAT"):
            type_file = candidate / "type"
            try:
                if type_file.read_text().strip().lower() == "battery":
                    yield candidate
            except FileNotFoundError:
                continue


def read_battery(path: Path) -> BatteryReading:
    energy_now_raw = _read_float(path / "energy_now")
    energy_full_raw = _read_float(path / "energy_full")
    energy_full_design_raw = _read_float(path / "energy_full_design")
    charge_now = _read_float(path / "charge_now")
    charge_full = _read_float(path / "charge_full")
    charge_full_design = _read_float(path / "charge_full_design")
    capacity_pct = _read_float(path / "capacity")
    status = _read_str(path / "status")
    voltage = _read_voltage(path)

    energy_now_wh = _wh_from_energy(energy_now_raw)
    energy_full_wh = _wh_from_energy(energy_full_raw)
    energy_full_design_wh = _wh_from_energy(energy_full_design_raw)

    if energy_now_wh is None:
        energy_now_wh = _energy_wh_from_charge(charge_now, voltage)
    if energy_full_wh is None:
        energy_full_wh = _energy_wh_from_charge(charge_full, voltage)
    if energy_full_design_wh is None:
        energy_full_design_wh = _energy_wh_from_charge(charge_full_design, voltage)

    percentage = None
    if energy_now_wh is not None and energy_full_wh:
        try:
            percentage = (energy_now_wh / energy_full_wh) * 100.0
        except ZeroDivisionError:
            percentage = None

    health_pct = None
    if energy_full_wh and energy_full_design_wh:
        try:
            health_pct = (energy_full_wh / energy_full_design_wh) * 100.0
        except ZeroDivisionError:
            health_pct = None

    return BatteryReading(
        path=path,
        capacity_pct=capacity_pct,
        percentage=percentage,
        energy_now_wh=energy_now_wh,
        energy_full_wh=energy_full_wh,
        energy_full_design_wh=energy_full_design_wh,
        health_pct=health_pct,
        status=status,
    )
