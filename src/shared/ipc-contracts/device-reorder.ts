/**
 * Wire contract for the `reorder_device_stories` Tauri command — the device
 * wheel order. Frontend mirror of `src-tauri/src/ipc/dto/device_reorder.rs`.
 */

export interface ReorderDeviceStoriesInput {
  deviceIdentifier: string;
  /** The COMPLETE list of the device's visible packs, in the new order. */
  orderedPackUuids: string[];
}

export interface ReorderDeviceStoriesOutcome {
  count: number;
  /** `false` when the device already listed that order (nothing written). */
  changed: boolean;
}

const DEVICE_ID = /^[0-9a-f]{32}$/;
const PACK_UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export function isReorderDeviceStoriesInput(
  value: unknown,
): value is ReorderDeviceStoriesInput {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.deviceIdentifier !== "string" || !DEVICE_ID.test(c.deviceIdentifier)) {
    return false;
  }
  if (!Array.isArray(c.orderedPackUuids) || c.orderedPackUuids.length === 0) {
    return false;
  }
  const seen = new Set<string>();
  for (const uuid of c.orderedPackUuids) {
    if (typeof uuid !== "string" || !PACK_UUID.test(uuid) || seen.has(uuid)) {
      return false;
    }
    seen.add(uuid);
  }
  return true;
}

export function isReorderDeviceStoriesOutcome(
  value: unknown,
): value is ReorderDeviceStoriesOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  return typeof c.count === "number" && typeof c.changed === "boolean";
}

/**
 * Move the pack `uuid` one step up (`-1`) or down (`+1`) in `order`, returning
 * the new order; the same array when the move has no room (already first /
 * last) or the uuid is absent — a stale double-click stays harmless.
 */
export function movePackUuid(
  order: readonly string[],
  uuid: string,
  direction: -1 | 1,
): string[] {
  const index = order.indexOf(uuid);
  const target = index + direction;
  if (index === -1 || target < 0 || target >= order.length) {
    return [...order];
  }
  const next = [...order];
  next[index] = order[target] as string;
  next[target] = uuid;
  return next;
}
