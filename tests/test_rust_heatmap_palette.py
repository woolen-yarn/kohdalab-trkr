"""Preserve the verified Python palette without retaining Qt or Python drivers."""
import hashlib
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
REFERENCE=json.loads((ROOT/'tests/fixtures/python_heatmap_reference.json').read_text())


def test_native_palette_matches_frozen_python_lookup_table():
    data=(ROOT/'next/crates/trkr-desktop/assets/rdbu_r_lut.rgb').read_bytes()
    assert len(data)==256*3
    assert hashlib.sha256(data).hexdigest()==REFERENCE['lut_sha256']


def test_native_index_mapping_matches_frozen_python_rgba():
    lut=(ROOT/'next/crates/trkr-desktop/assets/rdbu_r_lut.rgb').read_bytes()
    rgba=bytearray()
    for i in range(257):
        value=-1+i/128
        index=int(min(255,max(0,(value+1)*128)))
        rgba.extend(lut[3*index:3*index+3]);rgba.append(255)
    assert hashlib.sha256(rgba).hexdigest()==REFERENCE['rgba_257_sha256']
