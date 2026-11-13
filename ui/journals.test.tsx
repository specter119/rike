import { expect, test } from "nsh/test";
import { start } from "./testkit";

test("the journal stream shows today first, then older pages", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("text:十一月 周日", { timeout: 20000 });
  await app.waitFor("text:Meeting notes", { timeout: 20000 });
  const texts = app.texts();
  const today = texts.findIndex((t) => t.includes("十一月 周日"));
  const older = texts.findIndex((t) => t.includes("十月 周六"));
  expect(today).toBeGreaterThanOrEqual(0);
  expect(older).toBeGreaterThan(today);
});
