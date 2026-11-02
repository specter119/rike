import {
	computed,
	cleanup,
	effect,
	keymap,
	signal,
	Text,
	untracked,
	View,
} from "nsh";
import { Button, Checkbox, Input, TextView } from "nsh/base";
import type { TextViewLinkPressEvent } from "nsh/base";
import type { InputRef } from "nsh";
import { Rike } from "rike";
import type { OpResultDto, RowDto } from "rike";
import { renderMath } from "./math";
import {
	type Editing,
	clearFocusRequest,
	editing,
	endEdit,
	moveFocus,
	noteDraft,
	openBlock,
	openPageByTitle,
	runOp,
	setComposing,
	setTaskDone,
	startEdit,
	toggleCollapse,
} from "./store";

const INDENT = 24;

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

function Bullet(props: { rel: string; row: () => RowDto }) {
	const id = untracked(() => props.row().id);
	return (
		<Button
			testID="bullet"
			onPress={() => {
				if (props.row().hasChildren) toggleCollapse(props.rel, id);
			}}
			style={{ width: 18, flexShrink: 0, paddingVertical: 4 }}
		>
			<Text class="text-muted-foreground" style={{ fontSize: 14 }}>
				{props.row().hasChildren ? (props.row().collapsed ? "▸" : "▾") : "•"}
			</Text>
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
				<View style={{ width: 22, paddingVertical: 4 }}>
					<Checkbox
						testID="task-checkbox"
						checked={task() === "DONE"}
						onChange={(e) => setTaskDone(props.rel, id, e.checked)}
						indicatorClass="h-4 w-4 rounded border border-border items-center justify-center checked:bg-primary"
						indicator={
							<Text class="text-primary-foreground" style={{ fontSize: 11 }}>
								{task() === "DONE" ? "✓" : ""}
							</Text>
						}
					/>
				</View>
			</If>
			<If when={LABELLED.includes(task() ?? "")}>
				<Text
					class="text-muted-foreground"
					style={{ fontSize: 10, paddingVertical: 4, paddingRight: 4 }}
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
	// An editor that goes away mid-composition never hears composing=false.
	cleanup(() => setComposing(false));
	const empty = computed(() => text() === "");
	const single = computed(() => !text().includes("\n"));
	let caret: number | null = null;
	let input: InputRef | undefined;

	const mine = () => editing()?.epoch === edit.epoch;
	const op = (f: (g: number, t: string) => OpResultDto) =>
		runOp(rel, (g) => f(g, text()));

	if (edit.caretStart) {
		setTimeout(() => input?.setSelection(0, 0), 0);
	}
	clearFocusRequest();

	// Editing keys exist only while no input method is composing: the Linux keymap has no
	// composition guard, so a bound Enter/Backspace/arrow would eat the IME's keys.
	effect(() => {
		if (composing()) return;
		const offs = [
			keymap.bind("tab", () => op((g, t) => Rike.indent(rel, g, id, t)), {
				when: "always",
			}),
			keymap.bind(
				"shift+tab",
				() => op((g, t) => Rike.outdent(rel, g, id, t)),
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
				() => op((g, t) => Rike.toggleTodo(rel, g, id, t)),
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
				keymap.bind("up", () => moveFocus(rel, id, -1), { when: "always" }),
			);
			offs.push(
				keymap.bind("down", () => moveFocus(rel, id, 1), { when: "always" }),
			);
		}
		cleanup(() => offs.forEach((off) => off()));
	});

	return (
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
				if (mine()) noteDraft(rel, id, e.value);
			}}
			onSelectionChange={(e) => {
				caret = e.start;
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
	);
}
