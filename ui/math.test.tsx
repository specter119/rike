import { expect, test } from "nsh/test";
import { TODAY, start } from "./testkit";

const FORMULAS =
  "- inline \\(x^2\\) in text\n- \\[ \\frac{a}{b} \\]\n- broken $\\begin{$ here\n";

test("formulas are drawn and a broken one shows its TeX", async () => {
  const app = await start({ files: { [TODAY]: FORMULAS } });
  await app.waitUntil(() => app.count("id:math") >= 2, { timeout: 20000 });
  await app.waitUntil(() => app.count("id:math-fallback") >= 1);
  const texts = app.texts();
  expect(texts.some((t) => t.includes("frac"))).toBe(false);
  expect(texts.some((t) => t.includes("\\begin{"))).toBe(true);
});

test("pressing a formula edits the block as source", async () => {
  const app = await start({ files: { [TODAY]: FORMULAS } });
  await app.waitUntil(() => app.count("id:math") >= 1, { timeout: 20000 });
  await app.press("id:math >> nth=0");
  await app.waitUntil(() => app.count("id:block-input") === 1);
  expect(app.value("id:block-input")).toBe("inline \\(x^2\\) in text");
});
