from pathlib import Path

from battery_monitor import sysfs


def _write(path: Path, value: str) -> None:
    path.write_text(value)


def test_find_battery_paths(tmp_path: Path):
    bat0 = tmp_path / "BAT0"
    bat0.mkdir()
    _write(bat0 / "type", "Battery\n")
    ac = tmp_path / "AC"
    ac.mkdir()
    _write(ac / "type", "Mains\n")

    paths = list(sysfs.find_battery_paths(tmp_path))
    assert bat0 in paths
    assert ac not in paths


def test_read_battery(tmp_path: Path):
    bat = tmp_path / "BAT1"
    bat.mkdir()
    _write(bat / "type", "Battery\n")
    _write(bat / "energy_now", "40000000\n")  # 40 Wh
    _write(bat / "energy_full", "80000000\n")  # 80 Wh
    _write(bat / "energy_full_design", "90000000\n")  # 90 Wh
    _write(bat / "capacity", "95\n")
    _write(bat / "status", "Discharging\n")

    reading = sysfs.read_battery(bat)
    assert reading.energy_now_wh == 40
    assert reading.energy_full_wh == 80
    assert reading.energy_full_design_wh == 90
    assert round(reading.percentage, 2) == 50.0
    assert round(reading.health_pct, 2) == round(80 / 90 * 100, 2)
    assert reading.capacity_pct == 95
    assert reading.status == "Discharging"


def test_read_battery_uses_charge_and_voltage(tmp_path: Path):
    bat = tmp_path / "BAT0"
    bat.mkdir()
    _write(bat / "type", "Battery\n")
    _write(bat / "charge_now", "2000000\n")  # µAh
    _write(bat / "charge_full", "4000000\n")  # µAh
    _write(bat / "charge_full_design", "4500000\n")  # µAh
    _write(bat / "voltage_min_design", "11000000\n")  # µV
    _write(bat / "capacity", "90\n")
    _write(bat / "status", "Charging\n")

    reading = sysfs.read_battery(bat)
    assert reading.energy_now_wh == 22.0  # 2 Ah * 11 V
    assert reading.energy_full_wh == 44.0
    assert reading.energy_full_design_wh == 49.5
    assert round(reading.percentage or 0.0, 2) == 50.0
    assert round(reading.health_pct or 0.0, 2) == round(44 / 49.5 * 100, 2)
    assert reading.capacity_pct == 90
    assert reading.status == "Charging"


def test_read_battery_prefers_uevent_when_available(tmp_path: Path):
    bat = tmp_path / "BAT2"
    bat.mkdir()
    _write(
        bat / "uevent",
        "\n".join(
            [
                "POWER_SUPPLY_ENERGY_NOW=30000000",
                "POWER_SUPPLY_ENERGY_FULL=60000000",
                "POWER_SUPPLY_ENERGY_FULL_DESIGN=80000000",
                "POWER_SUPPLY_CAPACITY=85",
                "POWER_SUPPLY_STATUS=Discharging",
            ]
        ),
    )

    reading = sysfs.read_battery(bat)
    assert reading.energy_now_wh == 30.0
    assert reading.energy_full_wh == 60.0
    assert reading.energy_full_design_wh == 80.0
    assert round(reading.percentage or 0.0, 2) == 50.0
    assert round(reading.health_pct or 0.0, 2) == round(60 / 80 * 100, 2)
    assert reading.capacity_pct == 85
    assert reading.status == "Discharging"
