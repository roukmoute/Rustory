import type React from "react";
import { useOutletContext } from "react-router-dom";

type CollectionScreens = Record<"library" | "device", React.ReactNode>;

/** Each URL renders only its own screen; the parent keeps shared jobs alive. */
export function CollectionScreenRoute({
  screen,
}: {
  screen: keyof CollectionScreens;
}): React.ReactNode {
  return useOutletContext<CollectionScreens>()[screen];
}
