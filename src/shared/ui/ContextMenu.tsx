import type React from "react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";

import "./ContextMenu.css";

/** One actionable row of a context menu. `danger` tints a destructive
 *  action; `disabled` keeps the row visible with its reason unavailable. */
export interface ContextMenuItem {
  label: string;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
}

export interface ContextMenuProps {
  /** Viewport coordinates where the menu was invoked (the cursor). */
  x: number;
  y: number;
  items: ContextMenuItem[];
  /** Accessible name of the menu (e.g. the story title it acts on). */
  ariaLabel: string;
  /** Close without acting (Escape, outside click, scroll, blur). */
  onClose: () => void;
}

/**
 * A small, accessible right-click menu positioned at the cursor. Closes on
 * Escape, an outside click, a scroll or a window blur. Keyboard: ↑/↓ move,
 * Enter/Space activate, Escape closes. Rendered inline (no portal) with a
 * high `z-index`; kept within the viewport by a post-layout clamp.
 *
 * It owns NO domain logic — every row is a `{ label, onSelect }` provided by
 * the caller, so the same menu serves any surface.
 */
export function ContextMenu({
  x,
  y,
  items,
  ariaLabel,
  onClose,
}: ContextMenuProps): React.JSX.Element {
  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number }>({
    left: x,
    top: y,
  });
  // The index of the row with keyboard focus (−1 = none yet).
  const [activeIndex, setActiveIndex] = useState<number>(-1);

  const enabledIndexes = items
    .map((item, index) => (item.disabled ? -1 : index))
    .filter((index) => index >= 0);

  // Clamp inside the viewport once the real size is known (avoid spilling off
  // the right/bottom edge when invoked near a corner).
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const margin = 8;
    const left = Math.max(
      margin,
      Math.min(x, window.innerWidth - rect.width - margin),
    );
    const top = Math.max(
      margin,
      Math.min(y, window.innerHeight - rect.height - margin),
    );
    setPos({ left, top });
    // Move focus into the menu so keyboard users land on it.
    el.focus();
  }, [x, y]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    // A click/contextmenu/scroll/blur ANYWHERE closes the menu. `mousedown`
    // (not click) so it closes before a downstream click lands.
    const onOutside = (event: MouseEvent): void => {
      if (!menuRef.current?.contains(event.target as Node)) onClose();
    };
    const onScrollOrBlur = (): void => onClose();
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onOutside);
    document.addEventListener("contextmenu", onOutside);
    window.addEventListener("scroll", onScrollOrBlur, true);
    window.addEventListener("blur", onScrollOrBlur);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onOutside);
      document.removeEventListener("contextmenu", onOutside);
      window.removeEventListener("scroll", onScrollOrBlur, true);
      window.removeEventListener("blur", onScrollOrBlur);
    };
  }, [onClose]);

  const move = (delta: number): void => {
    if (enabledIndexes.length === 0) return;
    const current = enabledIndexes.indexOf(activeIndex);
    const next =
      current === -1
        ? enabledIndexes[delta > 0 ? 0 : enabledIndexes.length - 1]
        : enabledIndexes[
            (current + delta + enabledIndexes.length) % enabledIndexes.length
          ];
    setActiveIndex(next);
  };

  const activate = (index: number): void => {
    const item = items[index];
    if (!item || item.disabled) return;
    // Close first so the menu is gone before the action's own surface opens.
    onClose();
    item.onSelect();
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>): void => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      move(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      move(-1);
    } else if (event.key === "Enter" || event.key === " ") {
      if (activeIndex >= 0) {
        event.preventDefault();
        activate(activeIndex);
      }
    }
  };

  return (
    <div
      ref={menuRef}
      className="context-menu"
      role="menu"
      aria-label={ariaLabel}
      tabIndex={-1}
      style={{ left: pos.left, top: pos.top }}
      onKeyDown={handleKeyDown}
    >
      {items.map((item, index) => (
        <button
          key={item.label}
          type="button"
          role="menuitem"
          className={[
            "context-menu__item",
            item.danger ? "context-menu__item--danger" : null,
            index === activeIndex ? "context-menu__item--active" : null,
          ]
            .filter(Boolean)
            .join(" ")}
          aria-disabled={item.disabled || undefined}
          onMouseEnter={() => setActiveIndex(index)}
          onClick={() => activate(index)}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
