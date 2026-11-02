import { Rike } from "rike";
import { expect, test } from "nsh/test";
import { TODAY, file, start } from "./testkit";

const OLD = "journals/2026_10_31.md";

test("a clean page reloads when its file changes on disk", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("text:Meeting notes", { timeout: 20000 });
  Rike.devWrite(OLD, "- replaced from outside\n");
  await app.waitFor("text:replaced from outside", { timeout: 10000 });
  expect(app.shows("text:Meeting notes")).toBe(false);
});

test("a dirty page gets a notice instead of a reload", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("text:Meeting notes", { timeout: 20000 });
  await app.press("id:block-view >> nth=0");
  await app.waitFor("id:block-input");
  await app.type("id:block-input", "mine");
  Rike.devWrite(TODAY, "- theirs\n");
  await app.waitFor("text:载入外部版本", { timeout: 10000 });
  expect(app.shows("text:保留我的")).toBe(true);
  expect(app.value("id:block-input")).not.toBeNull();
  await app.type("id:block-input", " more");
  await app.wait(1200);
  expect(file(TODAY)).toBe("- theirs\n");
});

test("quit is refused while a page waits for the user to pick a side", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("text:Meeting notes", { timeout: 20000 });
  await app.press("id:block-view >> nth=0");
  await app.waitFor("id:block-input");
  await app.type("id:block-input", "mine");
  Rike.devWrite(TODAY, "- theirs\n");
  await app.waitFor("text:载入外部版本", { timeout: 10000 });
  await app.press("id:quit");
  await app.wait(600);
  expect(file(TODAY)).toBe("- theirs\n");
  expect(app.shows("text:载入外部版本")).toBe(true);
  await app.press("id:quit");
  await app.wait(300);
  expect(file(TODAY)).toBe("- theirs\n");
});
