from __future__ import annotations

import math
import re
from typing import Any, cast


_FLOAT_TOKEN = re.compile(r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[Ee][-+]?\d+)?")


def ensure_connected(inst: Any, model: str) -> None:
    try:
        connected = getattr(inst, "session", None) is not None
    except Exception:
        connected = False
    if not connected:
        raise ConnectionError(f"{model} VISA connection is closed")


def visa_write(inst: Any, model: str, command: str) -> None:
    ensure_connected(inst, model)
    try:
        inst.write(command)
    except Exception as exc:
        raise ConnectionError(
            f"{model} VISA write failed for {command}: {exc}"
        ) from exc


def visa_read(inst: Any, model: str, command: str) -> str:
    ensure_connected(inst, model)
    try:
        response = inst.read()
    except Exception as exc:
        raise ConnectionError(f"{model} VISA read failed for {command}: {exc}") from exc
    if isinstance(response, bytes):
        try:
            response = response.decode("ascii")
        except UnicodeDecodeError as exc:
            raise RuntimeError(
                f"{model} returned a non-ASCII response for {command}"
            ) from exc
    if not isinstance(response, str):
        raise RuntimeError(
            f"{model} returned an invalid response type for {command}: "
            f"{type(response).__name__}"
        )
    try:
        response.encode("ascii")
    except UnicodeEncodeError as exc:
        raise RuntimeError(
            f"{model} returned a non-ASCII response for {command}"
        ) from exc
    return response.strip()


def finite_float(value: object, *, context: str, input_value: bool = False) -> float:
    if isinstance(value, bool):
        error = ValueError if input_value else RuntimeError
        raise error(f"{context} must be finite, not boolean.")
    try:
        number = float(cast(Any, value))
    except (TypeError, ValueError) as exc:
        error = ValueError if input_value else RuntimeError
        raise error(f"Unexpected numeric value for {context}: {value!r}") from exc
    if not math.isfinite(number):
        error = ValueError if input_value else RuntimeError
        raise error(f"{context} must be finite.")
    return number


def parse_float_response(
    response: str,
    *,
    expected_count: int,
    cmd: str,
) -> list[float]:
    if isinstance(expected_count, bool) or not isinstance(expected_count, int):
        raise ValueError("expected_count must be a positive integer.")
    fields = [field.strip() for field in response.split(",")]
    if (
        expected_count < 1
        or len(fields) != expected_count
        or any(_FLOAT_TOKEN.fullmatch(field) is None for field in fields)
    ):
        raise RuntimeError(
            f"Unexpected response for {cmd}: {response!r} "
            f"(expected {expected_count} numeric values)"
        )
    return [finite_float(field, context=f"{cmd} response") for field in fields]


def integer_response(value: object, *, context: str) -> int:
    number = finite_float(value, context=context)
    if not number.is_integer():
        raise RuntimeError(f"Unexpected non-integer value for {context}: {number}")
    return int(number)


def resolve_index_from_table(
    value: object,
    table: dict[int, float],
    label: str,
) -> int:
    number = finite_float(value, context=label, input_value=True)
    best_index, best_value = min(
        table.items(),
        key=lambda item: abs(item[1] - number),
    )
    tolerance = max(1e-15, abs(best_value) * 1e-9)
    if abs(best_value - number) > tolerance:
        available = ", ".join(
            f"{available_value:.6g}" for available_value in table.values()
        )
        raise ValueError(
            f"Unsupported {label} value: {number}. Available values: {available}"
        )
    return best_index


def wait_time(multiplier: object, time_constant: object) -> float:
    factor = finite_float(multiplier, context="wait multiplier", input_value=True)
    if factor < 0:
        raise ValueError("wait multiplier must be non-negative.")
    constant = finite_float(time_constant, context="time constant")
    if constant < 0:
        raise RuntimeError("time constant must be non-negative.")
    return factor * constant
