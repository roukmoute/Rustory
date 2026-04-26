/**
 * Render an ISO-8601 timestamp as a calm, parent-friendly relative
 * time. Bands are coarse on purpose: the recovery banner is
 * informational, not a stopwatch.
 *
 * - `< 60 s`           → "à l'instant"
 * - `< 60 min`         → "il y a {N} minutes"
 * - `< 24 h`           → "il y a {N} heures"
 * - otherwise          → absolute date in `fr-FR` short form
 *
 * Falls back to a generic "récemment" if the timestamp cannot be parsed
 * — never throws, never returns the raw string (which would leak ISO
 * formatting into the UI).
 */
export function formatRelativeTime(
  iso: string,
  reference: Date = new Date(),
): string {
  const parsed = Date.parse(iso);
  if (Number.isNaN(parsed)) return "récemment";
  const rawDelta = reference.getTime() - parsed;
  // Negative delta means the recovery row carries a `draft_at`
  // strictly in the future relative to the current clock — clock
  // rollback, NTP correction, or a tampered DB. Saying "à l'instant"
  // would lie; saying "il y a -3 secondes" would be gibberish. Fall
  // back to an absolute date so the user sees something honest.
  if (rawDelta < 0) {
    return `le ${new Date(parsed).toLocaleDateString("fr-FR")}`;
  }
  // Use `Math.floor` consistently across all band boundaries so a
  // 59.5s delta does not round up into the "il y a 1 minute" band.
  const deltaSec = Math.floor(rawDelta / 1000);
  if (deltaSec < 60) return "à l'instant";
  const deltaMin = Math.floor(deltaSec / 60);
  if (deltaMin < 60) {
    return deltaMin === 1 ? "il y a 1 minute" : `il y a ${deltaMin} minutes`;
  }
  const deltaHour = Math.floor(deltaMin / 60);
  if (deltaHour < 24) {
    return deltaHour === 1 ? "il y a 1 heure" : `il y a ${deltaHour} heures`;
  }
  // 1 day or more — fall back to an absolute date the user can
  // recognize at a glance. `toLocaleDateString` is locale-aware so
  // months show in French.
  return `le ${new Date(parsed).toLocaleDateString("fr-FR")}`;
}
