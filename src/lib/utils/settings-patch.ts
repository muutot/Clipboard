export type SettingsPatch = Record<string, unknown>;

function isObject(value: unknown): value is SettingsPatch {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function keys(value: SettingsPatch): string[] {
  return Object.keys(value).filter(
    (key) => !["__proto__", "constructor", "prototype"].includes(key),
  );
}

/** Nested objects merge by field; arrays replace; null clears an optional field. */
export function diffSettings(before: object, after: object): SettingsPatch {
  const previous = before as SettingsPatch;
  const next = after as SettingsPatch;
  const patch: SettingsPatch = {};
  for (const key of new Set([...keys(previous), ...keys(next)])) {
    const oldValue = previous[key];
    const newValue = next[key];
    if (isObject(oldValue) && isObject(newValue)) {
      const nested = diffSettings(oldValue, newValue);
      if (Object.keys(nested).length) patch[key] = nested;
    } else if (JSON.stringify(oldValue) !== JSON.stringify(newValue)) {
      patch[key] = newValue === undefined ? null : newValue;
    }
  }
  return patch;
}

/** Compose queued edits without dropping deletion markers from an earlier edit. */
export function mergeSettingsPatches(earlier: SettingsPatch, later: SettingsPatch): SettingsPatch {
  const result = { ...earlier };
  for (const key of keys(later)) {
    const value = later[key];
    result[key] = isObject(value)
      ? mergeSettingsPatches(isObject(result[key]) ? result[key] : {}, value)
      : value;
  }
  return result;
}

export function applySettingsPatch(value: object, patch: SettingsPatch): SettingsPatch {
  const result: SettingsPatch = { ...value };
  for (const key of keys(patch)) {
    const update = patch[key];
    if (update === null) delete result[key];
    else if (isObject(update)) {
      result[key] = applySettingsPatch(isObject(result[key]) ? result[key] : {}, update);
    } else result[key] = update;
  }
  return result;
}
