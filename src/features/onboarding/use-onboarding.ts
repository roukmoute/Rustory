import { useCallback, useEffect, useRef, useState } from "react";

import {
  getOnboardingPreferences,
  updateOnboardingPreferences,
} from "../../ipc/commands/onboarding";
import { toAppError, type AppError } from "../../shared/errors/app-error";
import type { OnboardingPreferences } from "../../shared/ipc-contracts/onboarding";

/** Preferences only: device presence and inventory always come from their live hooks. */
export function useOnboarding() {
  const [preferences, setPreferences] = useState<OnboardingPreferences | null>(
    null,
  );
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const mounted = useRef(false);
  const generation = useRef(0);
  const knownPreferences = useRef<OnboardingPreferences | null>(null);
  const readInFlight = useRef(false);
  const saveInFlight = useRef(false);
  const retry = useCallback(() => {
    // A retry must not invalidate an acknowledged write or leave saving stuck.
    if (!mounted.current || saveInFlight.current) return;
    const token = ++generation.current;
    readInFlight.current = true;
    knownPreferences.current = null;
    setPreferences(null);
    setLoading(true);
    setError(null);
    void getOnboardingPreferences()
      .then((value) => {
        if (token !== generation.current) return;
        knownPreferences.current = value;
        setPreferences(value);
      })
      .catch((cause) => {
        if (token === generation.current) setError(toAppError(cause));
      })
      .finally(() => {
        if (token !== generation.current) return;
        readInFlight.current = false;
        setLoading(false);
      });
  }, []);
  useEffect(() => {
    mounted.current = true;
    retry();
    return () => {
      mounted.current = false;
      generation.current += 1;
      readInFlight.current = false;
    };
  }, [retry]);
  const complete = useCallback(
    async (dismissRecovery: boolean): Promise<boolean> => {
      const previous = knownPreferences.current;
      // A failed or unfinished read cannot supply the sibling flag for replacement.
      if (
        !mounted.current ||
        readInFlight.current ||
        saveInFlight.current ||
        !previous
      ) {
        return false;
      }
      saveInFlight.current = true;
      setSaving(true);
      setError(null);
      const token = ++generation.current;
      try {
        const next = await updateOnboardingPreferences({
          welcomeCompleted: true,
          deviceRecoveryPromptDismissed:
            dismissRecovery || previous.deviceRecoveryPromptDismissed,
        });
        if (token !== generation.current) return false;
        knownPreferences.current = next;
        setPreferences(next);
        return true;
      } catch (cause) {
        if (token === generation.current) setError(toAppError(cause));
        return false;
      } finally {
        saveInFlight.current = false;
        if (token === generation.current) setSaving(false);
      }
    },
    [],
  );
  return { preferences, loading, saving, error, retry, complete };
}
