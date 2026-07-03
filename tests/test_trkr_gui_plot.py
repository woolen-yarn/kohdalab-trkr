from __future__ import annotations

from kohdalab.apps.trkr_gui_plot import (
    sample_axis_ticks,
    scan2d_uses_equal_spatial_units,
    signal_monitor_top_labels,
    signal_scale,
    srkr_plot_series,
    standard_plot_series,
    trkr_top_labels,
)


def test_sample_axis_ticks_keeps_short_series():
    assert sample_axis_ticks([1.0, 2.0], ["a", "b"]) == [(1.0, "a"), (2.0, "b")]


def test_sample_axis_ticks_decimates_long_series_and_keeps_last():
    positions = [float(i) for i in range(25)]
    labels = [str(i) for i in range(25)]

    ticks = sample_axis_ticks(positions, labels, max_ticks=6)

    assert ticks[0] == (0.0, "0")
    assert ticks[-1] == (24.0, "24")
    assert len(ticks) <= 7


def test_signal_scale_leaves_theta_unscaled():
    assert signal_scale("X_V", 1000.0) == 1000.0
    assert signal_scale("Theta_deg", 1000.0) == 1.0


def test_scan2d_equal_spatial_units_only_for_xy():
    assert scan2d_uses_equal_spatial_units("x", "y")
    assert scan2d_uses_equal_spatial_units("y", "x")
    assert not scan2d_uses_equal_spatial_units("t", "x")


def test_standard_plot_series_for_signal_monitor():
    rows = [
        {"elapsed_s": 0.0, "X_V": 0.001, "Y_V": 0.002},
        {"elapsed_s": 1.0, "X_V": 0.003, "Y_V": 0.004},
    ]

    series1, series2 = standard_plot_series(
        rows,
        measurement_name="signal_monitor",
        signal1_key="X_V",
        signal2_key="Y_V",
        voltage_scale=1000.0,
    )

    assert series1.x == [0.0, 1.0]
    assert series1.y == [1.0, 3.0]
    assert series2.y == [2.0, 4.0]


def test_standard_plot_series_for_trkr_theta_mode():
    rows = [
        {"t_cor_ps": -10.0, "t_ps": -10.0, "R_V": 0.001, "Theta_deg": 12.0},
        {"t_cor_ps": 0.0, "t_ps": 0.0, "R_V": 0.002, "Theta_deg": 13.0},
    ]

    series1, series2 = standard_plot_series(
        rows,
        measurement_name="TRKR",
        signal1_key="R_V",
        signal2_key="Theta_deg",
        voltage_scale=1000.0,
    )

    assert series1.x == [-10.0, 0.0]
    assert series1.y == [1.0, 2.0]
    assert series2.y == [12.0, 13.0]


def test_srkr_plot_series_splits_axes_and_scales_signals():
    rows = [
        {"fast_axis": "x", "x_um": 10.0, "x_cor_um": 1.0, "R_V": 0.001, "Theta_deg": 12.0},
        {"fast_axis": "y", "y_um": 20.0, "y_cor_um": 2.0, "R_V": 0.002, "Theta_deg": 13.0},
        {"fast_axis": "x", "x_um": 30.0, "x_cor_um": 3.0, "R_V": 0.003, "Theta_deg": 14.0},
    ]

    series = srkr_plot_series(
        rows,
        signal1_key="R_V",
        signal2_key="Theta_deg",
        voltage_scale=1000.0,
    )

    assert series.x_signal1.positions == [10.0, 30.0]
    assert series.x_signal1.cor_values == [1.0, 3.0]
    assert series.x_signal1.signal_values == [1.0, 3.0]
    assert series.x_signal2.signal_values == [12.0, 14.0]
    assert series.y_signal1.positions == [20.0]
    assert series.y_signal1.signal_values == [2.0]


def test_top_axis_labels():
    assert signal_monitor_top_labels(3) == ["1", "2", "3"]
    assert trkr_top_labels([-122.0, -72.0, -22.0], -122.0) == ["0", "50", "100"]
