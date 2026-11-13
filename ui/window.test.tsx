import { expect, test } from "nsh/test";
import { start } from "./testkit";

test("the window controls sit in the header, next to the navigation", async () => {
  const app = await start();
  await app.waitFor("id:window-close");
  expect(app.shows("id:window-minimize")).toBe(true);
  expect(app.shows("id:window-maximize")).toBe(true);
  expect(app.shows("id:nav-back")).toBe(true);
  expect(app.shows("id:nav-forward")).toBe(true);
});

test("maximize, restore and minimize the window", async () => {
  const app = await start();
  await app.waitFor("id:window-maximize");
  await app.press("id:window-maximize");
  await app.waitUntil(() => app.window("main").state()?.mode === "maximized");
  await app.press("id:window-maximize");
  await app.waitUntil(() => app.window("main").state()?.mode === "normal");
  await app.press("id:window-minimize");
  await app.waitUntil(() => app.window("main").state()?.minimized === true);
  expect(app.window("main").state()?.minimized).toBe(true);
});

test("the app draws in the dark appearance", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("text:Meeting notes", { timeout: 20000 });
  await app.setColorScheme("dark");
  await app.wait(200);
  expect(app.shows("text:Meeting notes")).toBe(true);
  expect(app.shows("id:window-close")).toBe(true);
});
