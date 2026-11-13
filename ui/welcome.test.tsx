import { Rike } from "rike";
import { expect, test } from "nsh/test";
import { start, startWelcome } from "./testkit";

const dir = (name: string, files: Record<string, string>) =>
  Rike.devFolder(
    name,
    Object.entries(files).map(([path, text]) => ({ path, text })),
  );

test("with nothing to open the welcome page offers the recent vaults", async () => {
  const notes = dir("notes", { "journals/2026_10_01.md": "- hello from notes\n" });
  const app = await startWelcome({ recent: [notes], notice: "上次的库找不到了" });
  await app.waitFor("id:welcome");
  expect(app.shows("id:open-folder")).toBe(true);
  expect(app.shows("id:welcome-notice")).toBe(true);
  expect(app.count("id:recent-vault")).toBe(1);
  expect(app.shows("id:nav-journals")).toBe(false);
});

test("a recent vault opens in place, on its journals", async () => {
  const notes = dir("open", { "journals/2026_10_01.md": "- hello from notes\n" });
  const app = await startWelcome({ recent: [notes] });
  await app.press("id:recent-vault");
  await app.waitFor("text:hello from notes", { timeout: 10000 });
  expect(app.shows("id:welcome")).toBe(false);
  expect(app.shows("id:nav-journals")).toBe(true);
});

test("a folder that does not look like a vault needs confirmation", async () => {
  const other = dir("other", { "readme.txt": "not notes" });
  const app = await startWelcome({ recent: [other] });
  await app.press("id:recent-vault");
  await app.waitFor("id:confirm");
  await app.press("id:confirm-no");
  await app.waitUntil(() => app.count("id:confirm") === 0);
  expect(app.shows("id:welcome")).toBe(true);
  await app.press("id:recent-vault");
  await app.waitFor("id:confirm");
  await app.press("id:confirm-yes");
  await app.waitFor("id:nav-journals", { timeout: 10000 });
});

test("a missing folder is refused with a notice", async () => {
  const app = await startWelcome({ recent: ["/definitely/not/here"] });
  await app.press("id:recent-vault");
  await app.waitFor("id:leave-notice");
  expect(app.shows("id:welcome")).toBe(true);
});

test("the vault restores the page it was left on", async () => {
  const app = await start({
    seed: "basic",
    view: { kind: "page", rel: "pages/Inbox.md" },
  });
  await app.waitFor("text:two.one", { timeout: 20000 });
  expect(app.count("id:journal-card")).toBe(0);
});

test("the sidebar's vault menu opens", async () => {
  const app = await start({ seed: "basic" });
  await app.waitFor("id:vault-menu", { timeout: 20000 });
  await app.press("id:vault-menu");
  await app.waitFor("id:vault-open-folder");
});

test("the folder dialog can be cancelled without breaking the page", async () => {
  const app = await startWelcome();
  await app.press("id:open-folder");
  await app.wait(300);
  expect(app.shows("id:welcome")).toBe(true);
  expect(app.shows("id:open-folder")).toBe(true);
});

test("quit on the welcome page does not need a vault", async () => {
  const app = await startWelcome();
  await app.press("id:window-close");
  await app.wait(300);
  expect(app.shows("id:leave-notice")).toBe(false);
});
