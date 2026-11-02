import { Rike } from "rike";
import { expect, open } from "nsh/test";
import type { App } from "nsh/test";

export type Files = Record<string, string>;
export const TODAY = "journals/2026_11_01.md";

/** Starts the app over a fresh vault: the `basic` fixture (or none) plus `files`. */
export async function start(opts: { seed?: "basic"; files?: Files; view?: { kind: "journals" | "page"; rel?: string } } = {}): Promise<App> {
  const files = Object.entries(opts.files ?? {}).map(([path, text]) => ({ path, text }));
  Rike.devReset(opts.seed ?? null, files, opts.view ?? null);
  return open("./App.tsx");
}

/** Polls `read` until it equals `want` (the vault saves on a short delay). */
export async function untilFile(rel: string, want: string | null, timeout = 5000): Promise<void> {
  const end = Date.now() + timeout;
  let got: string | null = null;
  while (Date.now() < end) {
    got = Rike.devRead(rel) ?? null;
    if (got === want) return;
    await new Promise((r) => setTimeout(r, 50));
  }
  expect(got).toBe(want);
}

export const file = (rel: string): string | null => Rike.devRead(rel) ?? null;

/** Starts the app with no vault open (the welcome page). */
export async function startWelcome(opts: { recent?: string[]; notice?: string } = {}): Promise<App> {
  Rike.devWelcome(opts.recent ?? [], opts.notice ?? null);
  return open("./App.tsx");
}
