"""Validate the subset of JSON Schema used by the bundled report schema.

This is not a general JSON Schema implementation. Reject unknown constraints so
future schema changes cannot silently bypass validation. Diagnostics never echo
report values or user-controlled object keys.
"""

from __future__ import annotations

import datetime
import math


KEYWORDS = {
    "$schema", "title", "description", "type", "required", "properties",
    "items", "enum", "minimum", "maximum", "minItems", "maxItems",
    "additionalProperties", "format",
}


def validate(value: object, schema: dict, path: str = "$report") -> list[str]:
    unknown = set(schema) - KEYWORDS
    if unknown:
        raise ValueError("Unsupported schema keyword")
    kinds = schema.get("type")
    kinds = [kinds] if isinstance(kinds, str) else kinds
    checks = {
        "object": isinstance(value, dict),
        "array": isinstance(value, list),
        "string": isinstance(value, str),
        "number": type(value) is int or (type(value) is float and math.isfinite(value)),
        "integer": type(value) is int,
        "null": value is None,
        "boolean": type(value) is bool,
    }
    if kinds and not any(checks[kind] for kind in kinds):
        return [f"{path}: invalid type"]
    issues = []
    if "enum" in schema and value not in schema["enum"]:
        issues.append(f"{path}: invalid enum value")
    if type(value) in (int, float):
        if type(value) is float and not math.isfinite(value):
            issues.append(f"{path}: non-finite number")
        if "minimum" in schema and value < schema["minimum"]:
            issues.append(f"{path}: below minimum")
        if "maximum" in schema and value > schema["maximum"]:
            issues.append(f"{path}: above maximum")
    if isinstance(value, str) and schema.get("format") == "date":
        try:
            parsed = datetime.date.fromisoformat(value)
            if parsed.isoformat() != value:
                raise ValueError
        except ValueError:
            issues.append(f"{path}: invalid ISO date")
    if isinstance(value, dict):
        for key in schema.get("required", []):
            if key not in value:
                issues.append(f"{path}.{key}: required field missing")
        properties = schema.get("properties", {})
        for key, child in properties.items():
            if key in value:
                issues.extend(validate(value[key], child, f"{path}.{key}"))
        additional = schema.get("additionalProperties", True)
        for index, key in enumerate(key for key in value if key not in properties):
            if additional is False:
                issues.append(f"{path}: unexpected property")
            elif isinstance(additional, dict):
                issues.extend(validate(value[key], additional, f"{path}.additional[{index}]"))
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0):
            issues.append(f"{path}: too few items")
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            issues.append(f"{path}: too many items")
        for index, item in enumerate(value):
            if "items" in schema:
                issues.extend(validate(item, schema["items"], f"{path}[{index}]"))
    return issues
