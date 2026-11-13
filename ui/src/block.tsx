import {
	computed,
	cleanup,
	effect,
	keymap,
	signal,
	Text,
	token,
	untracked,
	View,
} from "nsh";
import { Button, Checkbox, Input, TextView } from "nsh/base";
import type { TextViewLinkPressEvent } from "nsh/base";
import type { InputRef } from "nsh";
import { Rike } from "rike";
import type { CompleteItemDto, OpResultDto, RowDto } from "rike";
import { renderMath } from "./math";
import {
	type Editing,
	editing,
	endEdit,
	moveFocus,
	noteDraft,
	openBlock,
	openPageByTitle,
	registerEditorText,
	unregisterEditorText,
	runOp,
	setComposing,
	setTaskDone,
	startEdit,
	toggleCollapse,
} from "./store";

const INDENT = 24;

/** How many candidates one completion panel shows; keep in step with Rust's `COMPLETE_LIMIT`. */
const COMPLETE_LIMIT = 8;

/** An open completion panel: the trigger's kind, where the query sits in the
 * text (UTF-16 units), and the candidates. */
type Completion = {
	kind: "page" | "block";
	from: number;
	to: number;
	/** What the range holds now: `replace` refuses if it changed. */
	query: string;
	items: CompleteItemDto[];
};

/** The completion trigger `text` has just before `caret` (`[[` or `((` not
 * yet closed), or null. `from`/`to` bracket the query. */
function triggerAt(
	text: string,
	caret: number,
): { kind: "page" | "block"; from: number; to: number } | null {
	for (let i = caret - 2; i >= 0; i--) {
		const pair = text.slice(i, i + 2);
		if (pair === "[[") return { kind: "page", from: i + 2, to: caret };
		if (pair === "((") return { kind: "block", from: i + 2, to: caret };
		const ch = text[i + 1];
		if (ch === "\n" || ch === "]" || ch === ")" || ch === "[" || ch === "(")
			return null;
	}
	return null;
}

/** A press on a `sx-page:` / `sx-block:` link of a block's Markdown. */
export function onLinkPress(e: TextViewLinkPressEvent): void {
	const link = Rike.link(e.url);
	if (!link) return;
	if (link.kind === "page") openPageByTitle(link.target);
	else openBlock(link.target);
}

export function Block(props: {
	rel: string;
	row: () => RowDto;
	readOnly: () => boolean;
}) {
	const id = untracked(() => props.row().id);
	// The editor is rebuilt (new epoch) whenever Rust changes the text under it.
	const editors = computed<Editing[]>(() => {
		const e = editing();
		return e && e.rel === props.rel && e.id === id ? [e] : [];
	});
	return (
		<View
			testID="block"
			style={{
				width: "100%",
				flexDirection: "row",
				paddingLeft: props.row().depth * INDENT,
			}}
		>
			<Bullet rel={props.rel} row={props.row} />
			<If when={editors().length === 0}>
				<TaskMarker rel={props.rel} row={props.row} />
			</If>
			<View style={{ flex: 1 }}>
				<If when={editors().length === 0}>
					<Button
						testID="block-view"
						onPress={() => {
							if (!props.readOnly()) startEdit(props.rel, id);
						}}
						style={{ width: "100%", minHeight: 26, paddingVertical: 4 }}
					>
						<TextView
							markdown={props.row().markdown}
							renderMath={renderMath}
							onLinkPress={onLinkPress}
						/>
					</Button>
				</If>
				<Each of={editors()} key={(e) => e.epoch}>
					{(e) => <Editor rel={props.rel} id={id} row={props.row} edit={e()} />}
				</Each>
			</View>
		</View>
	);
}

/** The mark at the start of a block: a dot on a leaf, a chevron on a block that folds. */
function Bullet(props: { rel: string; row: () => RowDto }) {
	const id = untracked(() => props.row().id);
	return (
		<Button
			testID="bullet"
			onPress={() => {
				if (props.row().hasChildren) toggleCollapse(props.rel, id);
			}}
			hoverStyle={{ opacity: 0.6 }}
			style={{ width: 18, flexShrink: 0, paddingTop: 7, alignItems: "flex-start" }}
		>
			<If
				when={props.row().hasChildren}
				else={
					<View
						style={{
							width: 5,
							height: 5,
							marginTop: 3,
							borderRadius: 3,
							backgroundColor: token("muted-foreground"),
							opacity: 0.5,
						}}
					/>
				}
			>
				<Text class="text-muted-foreground" style={{ fontSize: 9, lineHeight: 11 }}>
					{props.row().collapsed ? "▸" : "▾"}
				</Text>
			</If>
		</Button>
	);
}

const LABELLED = ["DOING", "NOW", "LATER", "WAIT"];

/** The checkbox (or state label) of a task block. A sibling of the text, so pressing it never starts editing. */
function TaskMarker(props: { rel: string; row: () => RowDto }) {
	const id = untracked(() => props.row().id);
	const task = () => props.row().task ?? null;
	return (
		<View style={{ flexDirection: "row", flexShrink: 0 }}>
			<If when={task() !== null && task() !== "CANCELED"}>
				<View style={{ width: 20, paddingTop: 5 }}>
					<Checkbox
						testID="task-checkbox"
						checked={task() === "DONE"}
						onChange={(e) => setTaskDone(props.rel, id, e.checked)}
						indicatorClass="h-4 w-4 rounded border border-input items-center justify-center checked:bg-foreground checked:border-foreground"
						indicator={
							<Text class="text-background" style={{ fontSize: 10 }}>
								{task() === "DONE" ? "✓" : ""}
							</Text>
						}
					/>
				</View>
			</If>
			<If when={LABELLED.includes(task() ?? "")}>
				<Text
					class="text-muted-foreground"
					style={{
						fontSize: 9.5,
						fontFamily: token("font-mono"),
						letterSpacing: 0.6,
						paddingTop: 6,
						paddingRight: 6,
					}}
				>
					{task() ?? ""}
				</Text>
			</If>
		</View>
	);
}

function Editor(props: {
	rel: string;
	id: string;
	row: () => RowDto;
	edit: Editing;
}) {
	const { rel, id, edit } = props;
	// The text is read once: the input owns it while it is mounted.
	const initial = untracked(() => props.row().text);
	const text = signal(initial);
	const composing = signal(false);
	// The store flushes drafts from here, so it always sees the input's
	// actual text, not just the last `onChange` value.
	registerEditorText(edit.epoch, () => text());
	// An editor that goes away mid-composition never hears composing=false.
	// A rebuild (new epoch) hands the composing flag to the new editor: only
	// reset the global one when this editor is still the current one.
	cleanup(() => {
		unregisterEditorText(edit.epoch);
		// A rebuild hands the composing flag to the new editor; a plain endEdit
		// (editing() already null) must still reset it.
		const cur = editing();
		if (cur && cur.epoch !== edit.epoch) return;
		setComposing(false);
		composing.set(false);
	});
	const empty = computed(() => text() === "");
	const single = computed(() => !text().includes("\n"));
	// The caret as far as we know it before the first selection event: a
	// rebuilt editor starts at `caret`/`caretStart`, a fresh one has none.
	let caret: number | null = edit.caret ?? (edit.caretStart ? 0 : null);
	let input: InputRef | undefined;

	const mine = () => editing()?.epoch === edit.epoch;
	const op = (f: (g: number, t: string) => OpResultDto) =>
		runOp(rel, (g) => f(g, text()));

	// Completion: opens on `[[` (pages) or `((` (blocks) with the caret inside,
	// closes when the trigger is gone.
	const compl = signal<Completion | null>(null);
	const complSel = signal(0);
	const updateCompl = (t: string, at: number) => {
		// Preedit text is not real block text: never open the panel on it, and
		// let an open panel sit still instead of chasing IME-internal offsets.
		if (composing()) return;
		const tr = triggerAt(t, at);
		if (!tr || (tr.kind === "block" && t.slice(tr.from, tr.to).trim() === "")) {
			compl.set(null);
			return;
		}
		try {
			const query = t.slice(tr.from, tr.to);
			const items = Rike.complete(query, tr.kind);
			compl.set(items.length ? { ...tr, query, items } : null);
			complSel.set(0);
		} catch {
			compl.set(null);
		}
	};
	const accept = (item: CompleteItemDto | undefined) => {
		const c = compl();
		// Accepting rebuilds the editor, which would cut an IME session short.
		if (!c || !item || !mine() || composing()) return;
		compl.set(null);
		// Through Rust: the editor is rebuilt from the model with the caret
		// after the inserted text. (`InputRef.insertText` cannot replace a
		// range in the headless runner, so the panel never relies on it.)
		runOp(rel, (g) =>
			Rike.replace(rel, g, id, c.from, c.to, c.query, item.insert),
		);
	};
	// The panel's keys. `escape` is left alone: it ends the edit (and the
	// panel with it), matching the shortcuts' single handler.
	effect(() => {
		if (composing()) return;
		const c = compl();
		if (!c) return;
		const n = Math.min(c.items.length, COMPLETE_LIMIT);
		const offs = [
			keymap.bind(
				"down",
				() => complSel.set((complSel() + 1) % n),
				{ when: "always" },
			),
			keymap.bind(
				"up",
				() => complSel.set((complSel() - 1 + n) % n),
				{ when: "always" },
			),
			keymap.bind("enter", () => accept(c.items[complSel()]), { when: "always" }),
			keymap.bind("tab", () => accept(c.items[complSel()]), { when: "always" }),
		];
		cleanup(() => offs.forEach((off) => off()));
	});

	if (edit.caretStart) {
		setTimeout(() => input?.setSelection(0, 0), 0);
	} else if (edit.caret !== undefined) {
		setTimeout(() => input?.setSelection(edit.caret!, edit.caret!), 0);
	}

	// Editing keys exist only while no input method is composing: the Linux keymap has no
	// composition guard, so a bound Enter/Backspace/arrow would eat the IME's keys.
	effect(() => {
		if (composing()) return;
		const offs = [
			keymap.bind(
				"tab",
				() => {
					if (!compl()) op((g, t) => Rike.indent(rel, g, id, t));
				},
				{ when: "always" },
			),
			keymap.bind(
				"shift+tab",
				() => {
					if (!compl()) op((g, t) => Rike.outdent(rel, g, id, t));
				},
				{ when: "always" },
			),
			keymap.bind(
				"alt+shift+up",
				() => op((g, t) => Rike.moveUp(rel, g, id, t)),
				{ when: "always" },
			),
			keymap.bind(
				"alt+shift+down",
				() => op((g, t) => Rike.moveDown(rel, g, id, t)),
				{ when: "always" },
			),
			keymap.bind(
				"ctrl+enter",
				() => {
					if (!compl()) op((g, t) => Rike.toggleTodo(rel, g, id, t));
				},
				{ when: "always" },
			),
		];
		if (empty()) {
			offs.push(
				keymap.bind(
					"backspace",
					() => op((g, t) => Rike.remove(rel, g, id, t)),
					{ when: "always" },
				),
			);
		}
		if (single()) {
			offs.push(
				keymap.bind(
					"up",
					() => {
						if (!compl()) moveFocus(rel, id, -1);
					},
					{ when: "always" },
				),
			);
			offs.push(
				keymap.bind(
					"down",
					() => {
						if (!compl()) moveFocus(rel, id, 1);
					},
					{ when: "always" },
				),
			);
		}
		cleanup(() => offs.forEach((off) => off()));
	});

	return (
		<View style={{ flexDirection: "column", width: "100%" }}>
		<Input
			testID="block-input"
			ref={(r: InputRef) => {
				input = r;
			}}
			multiline
			autoFocus
			defaultValue={initial}
			submitBehavior="submit"
			onChange={(e) => {
				text.set(e.value);
				if (mine()) {
					noteDraft(rel, id, e.value);
					updateCompl(e.value, caret ?? e.value.length);
				}
			}}
			onSelectionChange={(e) => {
				caret = e.start;
				updateCompl(text(), e.start);
			}}
			onCompositionChange={(e) => {
				composing.set(e.composing);
				setComposing(e.composing);
			}}
			onBlur={() => {
				if (mine()) endEdit();
			}}
			onSubmit={(e) => {
				if (!mine()) return;
				text.set(e.text);
				runOp(rel, (g) => Rike.split(rel, g, id, e.text, caret));
			}}
			style={{ width: "100%", padding: 4, minHeight: 26, fontSize: 14 }}
		/>
		<If when={compl()}>
			{(c) => (
				<View
					testID="complete-panel"
					style={{ flexDirection: "column", marginTop: 2 }}
				>
					<Each
						of={c().items.slice(0, COMPLETE_LIMIT)}
						key={(item: CompleteItemDto) => `${item.kind}:${item.label}:${item.detail}`}
					>
						{(item: () => CompleteItemDto, index: () => number) => (
							<Button
								testID="complete-item"
								onPress={() => accept(item())}
								style={{ paddingVertical: 2, paddingHorizontal: 8 }}
							>
								<Text
									class={
										index() === complSel() ? "text-primary" : "text-muted-foreground"
									}
									style={{ fontSize: 13 }}
								>
									{item().kind === "block"
										? `((${item().label}`
										: `[[${item().label}`}
									{item().detail ? ` — ${item().detail}` : ""}
								</Text>
							</Button>
						)}
					</Each>
				</View>
			)}
		</If>
		</View>
	);
}
