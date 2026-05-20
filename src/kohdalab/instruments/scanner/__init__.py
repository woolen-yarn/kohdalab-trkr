from .conexagap import ConexAgap
from .conexcc import ConexCC

SCANNER_CONTROLLERS = {
    "CONEXAGAP": ConexAgap,
    "CONEXCC": ConexCC,
}

__all__ = [
    "SCANNER_CONTROLLERS",
    "ConexAgap",
    "ConexCC",
]
