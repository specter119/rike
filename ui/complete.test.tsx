import { expect, test } from "nsh/test";
import { TODAY, file, start, untilFile } from "./testkit";

async function editFirst(app: Awaited<ReturnType<typeof start>>) {
	await app.waitFor("id:block-view");
	await app.press("id:block-view >> nth=0");
	await app.waitFor("id:block-input");
}

test("[[ offers page candidates and Enter inserts the link", async () => {
	const app = await start({ seed: "basic" });
	await editFirst(app);
	await app.type("id:block-input", "see [[In");
	await app.waitFor("id:complete-item");
	expect(file(TODAY) ?? "").not.toContain("[[Inbox");
	await app.keyboard.press("enter");
	await app.keyboard.press("escape");
	await untilFile(TODAY, "- see [[Inbox]] \n");
});

test("(( offers block candidates from the index", async () => {
	const app = await start({ seed: "basic" });
	await editFirst(app);
	await app.type("id:block-input", "ref ((想法一");
	await app.waitFor("id:complete-item");
	await app.keyboard.press("enter");
	await app.keyboard.press("escape");
	await untilFile(
		TODAY,
		"- ref ((6356e902-3b7b-4cb2-8c3e-6a904c813c40)) \n",
	);
});

test("down moves the selection and enter accepts the selected row", async () => {
	const app = await start({ seed: "basic" });
	await editFirst(app);
	await app.type("id:block-input", "[");
	await app.type("id:block-input", "[");
	await app.waitFor("id:complete-item");
	await app.keyboard.press("down");
	await app.keyboard.press("enter");
	await app.keyboard.press("escape");
	// The candidates sort to Inbox, Oct 31st, 项目/日课 (today's page is not
	// indexed yet): after one `down` the accepted one is October.
	await untilFile(TODAY, "- [[Oct 31st, 2026]] \n");
});
