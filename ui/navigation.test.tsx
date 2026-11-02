import { expect, test } from "nsh/test";
import { start } from "./testkit";

test("every journal day is its own card and opens as a page", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("text:Meeting notes", { timeout: 20000 });
  await app.waitUntil(() => app.count("id:journal-card") >= 2);
  expect(app.count("id:doc-path")).toBeGreaterThanOrEqual(2);
  await app.press("id:doc-title >> nth=1");
  await app.waitUntil(() => app.count("id:journal-card") === 0);
  await app.waitFor("text:Meeting notes");
});

test("back and forward walk the view history", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("text:Meeting notes", { timeout: 20000 });
  await app.waitFor("text:项目/日课");
  await app.press("text:项目/日课");
  await app.waitFor("text:想法一");
  expect(app.count("id:journal-card")).toBe(0);
  await app.back();
  await app.waitUntil(() => app.count("id:journal-card") > 0);
  await app.press("id:nav-forward");
  await app.waitFor("text:想法一");
  await app.press("id:nav-back");
  await app.waitUntil(() => app.count("id:journal-card") > 0);
  await app.keyboard.press("alt+right");
  await app.waitFor("text:想法一");
});

test("search finds pages, Escape closes the panel", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("text:Meeting notes", { timeout: 20000 });
  await app.waitFor("text:项目/日课");
  await app.press("text:项目/日课");
  await app.waitFor("text:想法一");
  expect(app.shows("text:个页面链接到这里")).toBe(false);
  await app.keyboard.press("ctrl+k");
  await app.waitFor("id:search-input");
  await app.type("id:search-input", "meeting");
  await app.waitFor("id:search-hit");
  await app.keyboard.press("escape");
  await app.waitUntil(() => app.count("id:search-input") === 0);
});

test("a page lists backlinks from a journal", async () => {
  const app = await start({
    files: { "pages/Inbox.md": "- one\n", "journals/2026_10_30.md": "- see [[Inbox]]\n" },
  });
  await app.waitFor("id:nav-page >> text:Inbox", { timeout: 10000 });
  await app.press("id:nav-page >> text:Inbox");
  await app.waitFor("text:1 个页面链接到这里");
  await app.waitFor("text:Oct 30th, 2026");
});
