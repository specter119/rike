// All UI state and actions. Rust (the `rike` native module) owns the data and the saving;
// this file owns what the screen shows: view, history, which block is edited, toasts.
import { batch, computed, signal } from "nsh";
import { Window } from "nsh/desktop";
import { Rike } from "rike";
import type {
	BacklinkGroupDto,
	BootDto,
	JournalEntryDto,
	EventDto,
	NoticeDto,
	OpResultDto,
	PageEntryDto,
	PageViewDto,
	ViewDto,
} from "rike";

export type View = { kind: "journals" } | { kind: "page"; rel: string };

const HISTORY_LIMIT = 100;
const JOURNAL_CHUNK = 7;
const DRAFT_DELAY_MS = 200;
const QUIT_SETTLE_MS = 100;

// ---- state ----
// Module-level signals live under the root owner. Anything made per page or per row
// would be disposed with the row that happened to create it.

const modeS = signal<"loading" | "welcome" | "vault">("loading");
const bootS = signal<BootDto | null>(null);
const startupErrorS = signal<string | null>(null);

const viewS = signal<View>({ kind: "journals" });
let backStack: View[] = [];
let forwardStack: View[] = [];
const canBackS = signal(false);
const canForwardS = signal(false);

const pagesRev = signal(0);
const pageMap = new Map<string, PageViewDto>();
const pageLoads = new Set<string>();

const journalsS = signal<{
	entries: JournalEntryDto[];
	more: boolean;
	loading: boolean;
}>({
	entries: [],
	more: false,
	loading: false,
});
let journalCount = JOURNAL_CHUNK;

const sidebarPagesS = signal<PageEntryDto[]>([]);
/** Bumps on every index change; for consumers that re-derive everything. */
const indexVerS = signal(0);
/** Page keys whose backlink groups changed in the last index event. */
const indexBacklinkKeysS = signal<readonly string[]>([]);
const toastsS = signal<NoticeDto[]>([]);
let localToastId = 0;

const searchOpenS = signal(false);
const quittingS = signal(false);
const leaveNoticeS = signal<string | null>(null);
const pendingConfirmS = signal<{ path: string; kind: "other" } | null>(null);

export type Editing = {
	rel: string;
	id: string;
	caretStart: boolean;
	/** An absolute caret (UTF-16) for a rebuilt editor; takes precedence. */
	caret?: number;
	epoch: number;
};
const editingS = signal<Editing | null>(null);
let editEpoch = 0;
let draft: {
	rel: string;
	id: string;
	text: string;
	generation: number;
} | null = null;
let draftTimer: number | undefined;

// The live input's text, registered by the mounted editor. Between `onChange`
// events the input's own text outranks the last captured value.
let editorText: { epoch: number; get: () => string } | null = null;

export function registerEditorText(epoch: number, get: () => string): void {
	editorText = { epoch, get };
}

export function unregisterEditorText(epoch: number): void {
	if (editorText?.epoch === epoch) editorText = null;
}

const composingS = signal(false);

let started = false;

// ---- exports: read side ----

export const mode: () => "loading" | "welcome" | "vault" = modeS;
export const boot: () => BootDto | null = bootS;
export const startupError: () => string | null = startupErrorS;
export const view: () => View = viewS;
export const canBack: () => boolean = canBackS;
export const canForward: () => boolean = canForwardS;
export const sidebarPages: () => PageEntryDto[] = sidebarPagesS;
export const indexVer: () => number = indexVerS;
export const toasts: () => NoticeDto[] = toastsS;
export const searchOpen: () => boolean = searchOpenS;
export const quitting: () => boolean = quittingS;
export const leaveNotice: () => string | null = leaveNoticeS;
export const pendingConfirm: () => { path: string; kind: "other" } | null =
	pendingConfirmS;
export const editing: () => Editing | null = editingS;
/** True while an input method is composing text: key bindings must not be mounted then. */
export const composing: () => boolean = composingS;
export function setComposing(on: boolean): void {
  composingS.set(on);
}
export const journals: () => {
	entries: JournalEntryDto[];
	more: boolean;
	loading: boolean;
} = journalsS;

/** The page's current state; first use starts loading it. */
export function pageOf(rel: string): () => PageViewDto | undefined {
	if (!pageMap.has(rel) && !pageLoads.has(rel)) void reloadPage(rel);
	// `computed` compares by identity, so a reader reruns only when its own page object changed.
	return computed(() => {
		pagesRev();
		return pageMap.get(rel);
	});
}

export function backlinksOf(rel: string): () => BacklinkGroupDto[] {
	return computed(() => {
		// The event's keys say which pages' references moved. A page's own
		// edits never change its own backlink group, so typing in the open
		// page no longer recomputes the panel.
		indexBacklinkKeysS();
		try {
			return Rike.backlinks(rel);
		} catch {
			return [];
		}
	});
}

// ---- errors and toasts ----

function message(e: unknown): string {
	return e instanceof Error ? e.message : String(e);
}

export function toast(text: string, kind: "info" | "error" = "error"): void {
	localToastId -= 1;
	toastsS.update((list) => [
		...list,
		{ id: localToastId, rel: "", message: text, kind },
	]);
}

export function dismissToast(id: number): void {
	toastsS.update((list) => list.filter((n) => n.id !== id));
	if (id > 0) {
		try {
			Rike.dismissNotice(id);
		} catch {
			// The session may be gone already.
		}
	}
}

function fail(e: unknown): void {
	toast(message(e));
}

// ---- pages ----

function putPage(page: PageViewDto): void {
	const old = pageMap.get(page.rel);
	// A slower, older load must not overwrite a newer page.
	if (old && old.generation > page.generation) return;
	pageMap.set(page.rel, page);
	pagesRev.update((n) => n + 1);
}

export async function reloadPage(rel: string): Promise<void> {
	pageLoads.add(rel);
	try {
		putPage(await Rike.page(rel));
	} catch (e) {
		fail(e);
	} finally {
		pageLoads.delete(rel);
	}
}

async function loadJournals(): Promise<void> {
	journalsS.update((j) => ({ ...j, loading: true }));
	try {
		const list = await Rike.journals(journalCount);
		journalsS.set({ entries: list.entries, more: list.more, loading: false });
	} catch (e) {
		journalsS.update((j) => ({ ...j, loading: false }));
		fail(e);
	}
}

export function loadMoreJournals(): void {
	journalCount += JOURNAL_CHUNK;
	void loadJournals();
}

function refreshPages(): void {
	try {
		sidebarPagesS.set(Rike.pages());
	} catch (e) {
		fail(e);
	}
}

// ---- events from Rust ----

function onEvent(ev: EventDto): void {
	switch (ev.kind) {
		case "page":
			if (ev.rel && pageMap.has(ev.rel)) void reloadPage(ev.rel);
			break;
		case "notice": {
			const n = ev.notice;
			if (!n) break;
			if (n.kind === "external" || n.kind === "conflict") {
				// Shown as the page's banner; fetch the page that carries it.
				if (n.rel) void reloadPage(n.rel);
			} else if (!toastsS().some((t) => t.id === n.id)) {
				toastsS.update((list) => [...list, n]);
			}
			break;
		}
		case "index": {
			if (ev.ver != null) indexVerS.set(ev.ver);
			// Only move the dirty-check signals when something actually moved:
			// setting an empty array would still wake every subscriber.
			const keys = ev.backlinkKeys ?? [];
			if (keys.length) indexBacklinkKeysS.set(keys);
			break;
		}
		case "pages":
			refreshPages();
			void loadJournals();
			break;
	}
}

// ---- start-up ----

export async function init(): Promise<void> {
	if (started) return;
	started = true;
	try {
		const b = Rike.boot();
		bootS.set(b);
		// Register before the session starts: events that came earlier are replayed.
		Rike.onEvent(onEvent);
		Rike.pump().catch(fail);
		if (b.mode === "vault") await enterVault(b);
		else modeS.set("welcome");
	} catch (e) {
		startupErrorS.set(message(e));
		modeS.set("welcome");
	}
}

async function enterVault(b: BootDto): Promise<void> {
	await Rike.start();
	pageMap.clear();
	backStack = [];
	forwardStack = [];
	journalCount = JOURNAL_CHUNK;
	batch(() => {
		canBackS.set(false);
		canForwardS.set(false);
		toastsS.set([]);
		pagesRev.update((n) => n + 1);
		viewS.set(
			b.view.kind === "page" && b.view.rel
				? { kind: "page", rel: b.view.rel }
				: { kind: "journals" },
		);
	});
	refreshPages();
	await loadJournals();
	modeS.set("vault");
	if (b.debugEditFirst) editFirstBlock();
}

function editFirstBlock(): void {
	const rel = viewS().kind === "page" ? (viewS() as { rel: string }).rel : journalsS().entries[0]?.rel;
	if (!rel) return;
	const poll = (tries: number) => {
		const row = pageMap.get(rel)?.rows[0];
		if (row) startEdit(rel, row.id);
		else if (tries > 0) setTimeout(() => poll(tries - 1), 100);
	};
	poll(30);
}

// ---- navigation ----

function sameView(a: View, b: View): boolean {
	return (
		a.kind === b.kind &&
		(a.kind === "journals" || (b.kind === "page" && a.rel === b.rel))
	);
}

function persistView(v: View): void {
	const dto: ViewDto = { kind: v.kind, rel: v.kind === "page" ? v.rel : null };
	try {
		Rike.setView(dto);
	} catch {
		// Remembering the view is best effort.
	}
}

function syncHistory(): void {
	canBackS.set(backStack.length > 0);
	canForwardS.set(forwardStack.length > 0);
}

export function navigate(v: View): void {
	if (quittingS()) return;
	endEdit();
	searchOpenS.set(false);
	const current = viewS();
	if (sameView(v, current)) return;
	backStack.push(current);
	if (backStack.length > HISTORY_LIMIT) backStack.shift();
	forwardStack = [];
	batch(() => {
		viewS.set(v);
		syncHistory();
	});
	persistView(v);
}

export function back(): void {
	if (quittingS()) return;
	const target = backStack.pop();
	if (!target) return;
	endEdit();
	searchOpenS.set(false);
	forwardStack.push(viewS());
	batch(() => {
		viewS.set(target);
		syncHistory();
	});
	persistView(target);
}

export function forward(): void {
	if (quittingS()) return;
	const target = forwardStack.pop();
	if (!target) return;
	endEdit();
	searchOpenS.set(false);
	backStack.push(viewS());
	batch(() => {
		viewS.set(target);
		syncHistory();
	});
	persistView(target);
}

export function openPageByTitle(title: string): void {
	try {
		navigate({ kind: "page", rel: Rike.relForTitle(title) });
	} catch (e) {
		fail(e);
	}
}

export function openBlock(id: string): void {
	try {
		const info = Rike.block(id);
		if (info) navigate({ kind: "page", rel: info.rel });
	} catch (e) {
		fail(e);
	}
}

// ---- search ----

export function openSearch(): void {
	if (modeS() === "vault" && !quittingS()) searchOpenS.set(true);
}

export function closeSearch(): void {
	searchOpenS.set(false);
}

// ---- editing ----

function currentGeneration(rel: string): number | undefined {
	return pageMap.get(rel)?.generation;
}

export function startEdit(rel: string, id: string, caretStart = false): void {
	if (quittingS()) return;
	const cur = editingS();
	if (cur && cur.rel === rel && cur.id === id) return;
	endEdit();
	const page = pageMap.get(rel);
	if (!page || page.readOnly) return;
	try {
		Rike.setEditing(rel, page.generation, id);
	} catch (e) {
		fail(e);
		return;
	}
	editEpoch += 1;
	editingS.set({ rel, id, caretStart, epoch: editEpoch });
}

/** Moves the editor's text into the Rust model and leaves editing. */
export function endEdit(): void {
	flushDraft();
	if (!editingS()) return;
	// Clear the Rust side first: only once that call has gone through do the
	// two sides agree. If it throws, the editor stays up and the user can
	// retry, instead of the states diverging.
	try {
		Rike.clearEditing();
	} catch {
		// No session: the Rust side holds no editing state either.
	}
	editingS.set(null);
}

/** Remembers the editor's latest text and sends it after a short pause. No redraw. */
export function noteDraft(rel: string, id: string, text: string): void {
	const generation = currentGeneration(rel);
	if (generation === undefined) return;
	draft = { rel, id, text, generation };
	if (draftTimer !== undefined) clearTimeout(draftTimer);
	draftTimer = setTimeout(() => {
		draftTimer = undefined;
		flushDraft();
	}, DRAFT_DELAY_MS) as unknown as number;
}

/** Sends the pending draft now. Returns its text, or undefined when there was none. */
export function flushDraft(): string | undefined {
	if (draftTimer !== undefined) {
		clearTimeout(draftTimer);
		draftTimer = undefined;
	}
	const d = draft;
	if (!d) return undefined;
	draft = null;
	// The live input is the authority: its current text outranks the last
	// captured `onChange` value, which can lag behind on Linux. It is only
	// trusted for the same block the draft belongs to.
	const live = editingS();
	const text =
		live &&
		editorText &&
		editorText.epoch === live.epoch &&
		live.rel === d.rel &&
		live.id === d.id
			? editorText.get()
			: d.text;
	try {
		if (!Rike.setDraft(d.rel, d.generation, d.id, text)) {
			toast("页面已从磁盘重新载入，未保存的输入被丢弃", "info");
		}
	} catch (e) {
		fail(e);
	}
	return text;
}

function applyOp(res: OpResultDto): void {
	batch(() => {
		putPage(res.page);
		const focus = res.focus;
		if (focus) {
			try {
				Rike.setEditing(res.page.rel, res.page.generation, focus.id);
			} catch (e) {
				fail(e);
				return;
			}
			// A new epoch rebuilds the editor, so it shows the text Rust now holds.
			editEpoch += 1;
			editingS.set({
				rel: res.page.rel,
				id: focus.id,
				caretStart: focus.caretStart,
				caret: focus.caret ?? undefined,
				epoch: editEpoch,
			});
		}
	});
}

/** Moves the editor to the previous (-1) or next (1) visible block of the page. */
export function moveFocus(rel: string, id: string, delta: number): void {
  const rows = pageMap.get(rel)?.rows;
  if (!rows) return;
  const at = rows.findIndex((r) => r.id === id);
  const target = rows[at + delta];
  if (at >= 0 && target) startEdit(rel, target.id, delta > 0);
}

/** Runs a structure operation on a page. `f` gets the page's current generation. */
/** The page the last structure edit touched: what Ctrl+Z undoes from the Journals stream. */
let lastOpRel: string | null = null;

/** The page Ctrl+Z / Ctrl+Shift+Z act on: the open page, else the last one edited. */
export function undoTarget(): string | null {
	const v = viewS();
	return v.kind === "page" ? v.rel : lastOpRel;
}

export function runOp(
	rel: string,
	f: (generation: number) => OpResultDto,
): void {
	if (quittingS()) return;
	flushDraft();
	const generation = currentGeneration(rel);
	if (generation === undefined) return;
	lastOpRel = rel;
	try {
		applyOp(f(generation));
	} catch (e) {
		fail(e);
	}
}

export function undo(rel: string): void {
	if (editingS()) return;
	runOp(rel, (g) => Rike.undo(rel, g));
}

export function redo(rel: string): void {
	if (editingS()) return;
	runOp(rel, (g) => Rike.redo(rel, g));
}

export function setTaskDone(rel: string, id: string, done: boolean): void {
	runOp(rel, (g) => Rike.setTaskDone(rel, g, id, done));
}

export function toggleCollapse(rel: string, id: string): void {
	runOp(rel, (g) => Rike.toggleCollapse(rel, g, id));
}

export function acceptDisk(rel: string): void {
	if (quittingS()) return;
	endEdit();
	Rike.acceptDisk(rel).then(putPage).catch(fail);
}

export function keepMine(rel: string): void {
	if (quittingS()) return;
	try {
		Rike.keepMine(rel);
	} catch (e) {
		fail(e);
		return;
	}
	void reloadPage(rel);
}

// ---- quitting and switching vaults ----

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

async function leave(verb: "quit" | "switch", path?: string): Promise<void> {
	if (quittingS()) return;
	leaveNoticeS.set(null);
	searchOpenS.set(false);
	// Disable the UI first, then give the last `onChange` a moment to arrive.
	quittingS.set(true);
	try {
		await sleep(QUIT_SETTLE_MS);
		flushDraft();
		endEdit();
		const result = await Rike.leave(verb, path ?? null);
		if (!result.ok) {
			leaveNoticeS.set(result.notice ?? "无法退出");
			quittingS.set(false);
		}
	} catch (e) {
		leaveNoticeS.set(message(e));
		quittingS.set(false);
	}
}

export function quit(): Promise<void> {
	if (modeS() !== "vault") {
		// Nothing is open, so there is nothing to save.
		Window.get("main")?.close();
		return Promise.resolve();
	}
	return leave("quit");
}

async function openChecked(path: string): Promise<void> {
	if (modeS() === "vault") {
		await leave("switch", path);
		return;
	}
	try {
		const b = await Rike.open(path);
		bootS.set(b);
		await enterVault(b);
	} catch (e) {
		leaveNoticeS.set(message(e));
	}
}

export async function switchVault(path: string): Promise<void> {
	if (quittingS()) return;
	leaveNoticeS.set(null);
	pendingConfirmS.set(null);
	try {
		const found = await Rike.inspect(path);
		if (found.reason) {
			leaveNoticeS.set(found.reason);
			return;
		}
		const canonical = found.canonical ?? path;
		if (found.isCurrent) {
			leaveNoticeS.set("这已经是当前打开的库");
			return;
		}
		if (found.kind === "other") {
			pendingConfirmS.set({ path: canonical, kind: "other" });
			return;
		}
		await openChecked(canonical);
	} catch (e) {
		leaveNoticeS.set(message(e));
	}
}

export function confirmOpen(yes: boolean): void {
	const pending = pendingConfirmS();
	pendingConfirmS.set(null);
	if (yes && pending) void openChecked(pending.path);
}

export async function pickAndSwitch(): Promise<void> {
	if (quittingS()) return;
	try {
		const picked = await Rike.pickFolder(bootS()?.root ?? null);
		if (picked) await switchVault(picked);
	} catch (e) {
		leaveNoticeS.set(message(e));
	}
}
