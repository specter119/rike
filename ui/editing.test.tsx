import { expect, test } from "nsh/test";
import { TODAY, file, start, untilFile } from "./testkit";

async function editFirst(app: Awaited<ReturnType<typeof start>>) {
  await app.waitFor("id:block-view");
  await app.press("id:block-view >> nth=0");
  await app.waitFor("id:block-input");
}

test("typing in today's journal creates the file", async () => {
  const app = await start();
  await editFirst(app);
  await app.type("id:block-input", "hello [[Inbox]]");
  await app.keyboard.press("escape");
  await untilFile(TODAY, "- hello [[Inbox]]\n");
});

test("Enter splits the block and Tab indents it", async () => {
  const app = await start();
  await editFirst(app);
  await app.type("id:block-input", "parent");
  await app.submit("id:block-input");
  await app.waitUntil(() => app.count("id:block") >= 2);
  await app.type("id:block-input", "child");
  await app.keyboard.press("tab");
  await app.keyboard.press("escape");
  await untilFile(TODAY, "- parent\n\t- child\n");
});

test("Ctrl+Enter toggles TODO", async () => {
  const app = await start();
  await editFirst(app);
  await app.type("id:block-input", "task");
  await app.keyboard.press("ctrl+enter");
  await app.waitFor("id:block-input");
  await app.keyboard.press("escape");
  await untilFile(TODAY, "- TODO task\n");
});

test("Backspace in an empty block deletes it", async () => {
  const app = await start();
  await editFirst(app);
  await app.type("id:block-input", "a");
  await app.submit("id:block-input");
  await app.waitUntil(() => app.count("id:block") >= 2);
  await app.keyboard.press("backspace");
  await app.waitUntil(() => app.count("id:block") === 1);
  await app.keyboard.press("escape");
  await untilFile(TODAY, "- a\n");
});

test("Ctrl+Z undoes a structural edit and Ctrl+Shift+Z redoes it", async () => {
  const app = await start();
  await editFirst(app);
  await app.type("id:block-input", "a");
  await app.submit("id:block-input");
  await app.waitUntil(() => app.count("id:block") >= 2);
  await app.type("id:block-input", "b");
  await app.keyboard.press("escape");
  await untilFile(TODAY, "- a\n- b\n");
  await app.keyboard.press("ctrl+z");
  await untilFile(TODAY, "- a\n");
  await app.keyboard.press("ctrl+shift+z");
  await untilFile(TODAY, "- a\n- b\n");
});

test("a task checkbox toggles without editing", async () => {
  const app = await start({ files: { [TODAY]: "- TODO ship\n" } });
  await app.waitFor("id:task-checkbox");
  await app.press("id:task-checkbox >> nth=0");
  await untilFile(TODAY, "- DONE ship\n");
  expect(app.count("id:block-input")).toBe(0);
  await app.press("id:task-checkbox >> nth=0");
  await untilFile(TODAY, "- TODO ship\n");
  expect(app.count("id:block-input")).toBe(0);
});

test("quit saves a draft without waiting for the debounce", async () => {
  const app = await start();
  await editFirst(app);
  await app.type("id:block-input", "last words");
  await app.press("id:quit");
  await app.wait(380);
  expect(file(TODAY)).toBe("- last words\n");
  expect(app.count("id:block-input")).toBe(0);
});

test("quit leaves an untouched journal unwritten", async () => {
  const app = await start();
  await app.waitFor("text:Nov 1st, 2026");
  await app.keyboard.press("ctrl+w");
  await app.wait(500);
  expect(file(TODAY)).toBeNull();
});

test("the window's close button saves and quits", async () => {
  const app = await start();
  await editFirst(app);
  await app.type("id:block-input", "bye");
  await app.press("id:window-close");
  await app.wait(380);
  expect(file(TODAY)).toBe("- bye\n");
});
