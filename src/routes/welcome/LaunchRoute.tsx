import { Link, Navigate } from "react-router-dom";
import { useOnboarding } from "../../features/onboarding/use-onboarding";
import { Button, ProgressIndicator } from "../../shared/ui";

/** Only the app launch chooses welcome; direct collection links stay direct. */
export function LaunchRoute() {
  const onboarding = useOnboarding();
  if (onboarding.loading)
    return (
      <main aria-label="Ouverture de Rustory">
        <ProgressIndicator mode="indeterminate" label="Ouverture de Rustory…" />
      </main>
    );
  if (onboarding.preferences)
    return (
      <Navigate
        to={onboarding.preferences.welcomeCompleted ? "/library" : "/welcome"}
        replace
      />
    );
  return (
    <main className="settings-route" aria-label="Ouverture de Rustory">
      <h1>Bienvenue sur Rustory</h1>
      <p role="alert">Les préférences d'accueil n'ont pas pu être lues.</p>
      <Button onClick={onboarding.retry}>Réessayer</Button>
      <Link to="/library">Ouvrir ma bibliothèque</Link>
    </main>
  );
}
