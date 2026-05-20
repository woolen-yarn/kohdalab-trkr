from __future__ import annotations

import pytest

from kohdalab.api.notebook import make_srkr_2d_live_update, make_srkr_live_update, make_strkr_live_update


def test_srkr_live_update_accepts_fast_axis_alias(monkeypatch):
    calls = []
    monkeypatch.setattr(
        "kohdalab.api.notebook.make_trkr_live_update",
        lambda **kwargs: calls.append(kwargs) or (lambda _point: None),
    )

    make_srkr_live_update(fast_axis="y", y_key="R_V")

    assert calls == [
        {
            "x_key": "y_cor_um",
            "y_key": "R_V",
            "xlabel": None,
            "ylabel": None,
            "title": None,
        }
    ]


def test_strkr_live_update_uses_t_or_spatial_fast_axis(monkeypatch):
    calls = []
    monkeypatch.setattr(
        "kohdalab.api.notebook.make_trkr_live_update",
        lambda **kwargs: calls.append(kwargs) or (lambda _point: None),
    )

    make_strkr_live_update(fast_axis="t")
    make_strkr_live_update(fast_axis="x")

    assert [call["x_key"] for call in calls] == ["t_cor_ps", "x_cor_um"]


def test_srkr_2d_live_update_rejects_t_fast_axis():
    with pytest.raises(ValueError, match="fast_axis"):
        make_srkr_2d_live_update(fast_axis="t")
