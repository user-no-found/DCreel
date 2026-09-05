import type { Dashboard, FencePatch, Preferences } from "../types";

export function clearMatchingPatch<T extends object>(
  optimistic: Partial<T>,
  completed: Partial<T>
): Partial<T> {
  const remaining = { ...optimistic };
  for (const key of Object.keys(completed) as (keyof T)[]) {
    if (Object.is(remaining[key], completed[key])) {
      Reflect.deleteProperty(remaining, key);
    }
  }
  return remaining;
}

export function mergeDashboardWithOptimistic(
  loaded: Dashboard,
  preferences: Partial<Preferences>,
  fences: ReadonlyMap<string, FencePatch>
): Dashboard {
  return {
    ...loaded,
    preferences: { ...loaded.preferences, ...preferences },
    fences: loaded.fences.map((fence) => ({
      ...fence,
      ...(fences.get(fence.id) ?? {})
    }))
  };
}
