from .gsc01 import GSC01
from .gsc01a import GSC01A
from .shot302 import Shot302GS

DELAY_STAGE_CONTROLLERS = {
    "GSC01": GSC01,
    "GSC01A": GSC01A,
    "SHOT302GS": Shot302GS,
}

__all__ = [
    "DELAY_STAGE_CONTROLLERS",
    "GSC01",
    "GSC01A",
    "Shot302GS",
]
