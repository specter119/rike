import { cleanup, effect, keymap } from "nsh";
import { BackHandler } from "nsh/mobile";
import {
  back,
  canBack,
  closeSearch,
  composing,
  endEdit,
  forward,
  openSearch,
  quit,
  redo,
  searchOpen,
  undo,
  undoTarget,
} from "./store";

/**
 * Global shortcuts. Call once from the root component's body. While an input method
 * composes nothing is bound: the Linux keymap has no composition guard of its own.
 */
export function installShortcuts(): void {
  // Registered outside the effect: it must survive composition.
  BackHandler.on("press", () => {
    if (!canBack()) return false;
    back();
    return true;
  });

  const pageUndo = (run: (rel: string) => void) => () => {
    const rel = undoTarget();
    if (rel) run(rel);
  };

  effect(() => {
    if (composing()) return;
    const offs = [
      keymap.bind("ctrl+k", () => (searchOpen() ? closeSearch() : openSearch()), { when: "always" }),
      keymap.bind("ctrl+w", () => void quit(), { when: "always" }),
      keymap.bind("alt+right", forward, { when: "always" }),
      keymap.bind("escape", () => (searchOpen() ? closeSearch() : endEdit()), { when: "always" }),
      keymap.bind("ctrl+z", pageUndo(undo), { when: "notEditing" }),
      keymap.bind("ctrl+shift+z", pageUndo(redo), { when: "notEditing" }),
    ];
    cleanup(() => offs.forEach((off) => off()));
  });
}
