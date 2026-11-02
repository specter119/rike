import { expect, open, test } from "nsh/test";
import { Pump } from "probe/native";

test("swap: old Stop does not clear the new callback", async () => {
  const app = await open("./App.tsx");
  await app.press("swap");
  Pump.spawnSource(0, 3);
  await app.waitFor("text:b 3", { timeout: 3000 });
  expect(app.texts()).toContain("a 0");
});

test("events buffered with no callback replay on registration", async () => {
  const app = await open("./App.tsx");
  await app.press("unreg-a");
  Pump.spawnSource(0, 4);
  await app.wait(300);
  expect(app.texts()).toContain("a 0");
  expect(app.texts()).toContain("b 0");
  await app.press("reg-b");
  await app.waitFor("text:b 4", { timeout: 3000 });
});

test("main-owned runtime from async and sync members", async () => {
  expect(await Pump.onRuntime()).toBe(42);
  const app = await open("./App.tsx");
  await app.press("unreg-a");
  await app.press("reg-b");
  Pump.spawnOnRuntime(7);
  await app.waitFor("text:b 1", { timeout: 3000 });
});
