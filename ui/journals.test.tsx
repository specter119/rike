import { expect, test } from "nsh/test";
import { start } from "./testkit";

test("the journal stream shows today first, then older pages", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("text:Nov 1st, 2026", { timeout: 20000 });
  await app.waitFor("text:Meeting notes", { timeout: 20000 });
  const texts = app.texts();
  const today = texts.findIndex((t) => t.includes("Nov 1st, 2026"));
  const older = texts.findIndex((t) => t.includes("Oct 31st, 2026"));
  expect(today).toBeGreaterThanOrEqual(0);
  expect(older).toBeGreaterThan(today);
});
