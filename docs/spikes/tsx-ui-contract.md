# TSX UI 分工契约

`ui/` 的文件归属和模块之间的接口。多个 worker 并行写，只改自己的文件。

## 约定

- 原生模块 `rike`（`crates/rike-native/src/api.rs`）：TSX 里 `import { Rike } from "rike"`，DTO 类型从 `"rike"` 导入（`nsh check` 生成 `ui/.nsh/types/rike.d.ts`）。
- 同步方法返回值，异步方法返回 Promise，抛错就是 reject / throw，用 `try/catch`。
- 所有 `rel`/`generation`/`id` 原样透传，页面操作必须带当前页面的 `generation`。
- 文件间只通过下面列出的导出通信。

## 文件归属

| 文件 | 负责 | 说明 |
| --- | --- | --- |
| `ui/src/store.ts` | W1 | 全部状态和动作（下面两节的导出） |
| `ui/src/{block,editor,page,journals}.tsx` | W2 | 大纲主体：Journals 卡片流、页面、块行、编辑器、页面横幅（冲突/外部修改按钮） |
| `ui/App.tsx`、`ui/src/{welcome,sidebar,notices}.tsx` | W3 | 组装、欢迎页、侧栏（含切库菜单、退出按钮）、toast |
| `ui/src/{search,shortcuts,backlinks,math}.ts(x)` | W4 | 搜索浮层、全局快捷键、反链面板、公式渲染回调 |
| `*.test.tsx` | 各自 | `nsh test` headless 测试，文件名跟着被测文件 |

组件导出名固定：`Journals`（journals.tsx）、`Page({rel})`（page.tsx）、`Block`/`Editor`（内部）、`Backlinks({rel})`（backlinks.tsx，page.tsx 用）、`SearchOverlay`（search.tsx）、`Sidebar`、`Welcome`、`Toasts`、`installShortcuts()`（shortcuts.ts，在 App 里调用一次）、`renderMath`（math.ts，签名 `(tex: string, display: boolean) => FormulaDto | null`，内部取暗色模式，TextView 的 `renderMath` 回调用它）。

## store.ts 导出（A 实现，B 使用）

```ts
import type { BootDto, NoticeDto, PageEntryDto, PageViewDto, SearchHitDto } from "rike";

export type View = { kind: "journals" } | { kind: "page"; rel: string };

// 启动
export function init(): Promise<void>;          // boot → pump → start → on_event → 恢复视图；在 App 的 mount 里调用一次
export const mode: () => "loading" | "welcome" | "vault";
export const boot: () => BootDto | null;        // recent / notice / root
export const startupError: () => string | null;

// 导航（所有换页都走 navigate：先 endEdit、关搜索，视图不同才入栈并清空 forward，上限 100）
export const view: () => View;
export function navigate(v: View): void;
export function back(): void;
export function forward(): void;
export const canBack: () => boolean;
export const canForward: () => boolean;
export function openPageByTitle(title: string): void;   // `[[x]]` 链接

// 页面数据
export const sidebarPages: () => PageEntryDto[];        // Rike.pages()，随 "pages" 事件刷新
export const indexVer: () => number;                    // 随 "index" 事件递增，搜索/反链依赖它

// 提示
export const toasts: () => NoticeDto[];                 // info/error 类，可 dismissToast
export function dismissToast(id: number): void;

// 搜索（B 画 UI，A 提供状态）
export const searchOpen: () => boolean;
export function openSearch(): void;
export function closeSearch(): void;

// 退出 / 切库（leave 流程在 A 里：inspect → quitting → 100ms → setDraft → clearEditing → Rike.leave）
export const quitting: () => boolean;                   // 为 true 时界面禁用输入
export const leaveNotice: () => string | null;          // 被拒绝时的原因，显示在侧栏/欢迎页
export function quit(): Promise<void>;
export function switchVault(path: string): Promise<void>;
export function pickAndSwitch(): Promise<void>;         // 文件夹对话框 → inspect → (非 Logseq 目录要确认) → switchVault / welcome 下原地 open
export const pendingConfirm: () => { path: string; kind: "other" } | null;  // 需要用户确认的非 Logseq 目录
export function confirmOpen(yes: boolean): void;
```

欢迎页（mode==="welcome"）里的打开走 `Rike.open(path)` + `Rike.start()` 原地进入，不重启进程；已经有库时切库走 `leave("switch", path)`（重启进程）。两条路径都由 `switchVault`/`pickAndSwitch` 内部区分。

## A 的页面接口（供 B 的 search/sidebar 点击使用）

- `navigate({kind:"page", rel})`、`navigate({kind:"journals"})`。
- 点击搜索结果里的块：`navigate` 到该页（块高亮不做）。

## store.ts 的页面与编辑接口（W1 实现，W2/W4 使用）

```ts
import type { BacklinkGroupDto, JournalEntryDto, OpResultDto, PageViewDto } from "rike";

export const pageOf: (rel: string) => () => PageViewDto | undefined;   // 每页一个信号，第一次调用时异步加载；"page" 事件按 generation 规则刷新
export const journals: () => { entries: JournalEntryDto[]; more: boolean; loading: boolean };
export function loadMoreJournals(): void;

// 编辑状态（任一时刻最多一个块）
export const editing: () => { rel: string; id: string; caretStart: boolean } | null;
export function startEdit(rel: string, id: string, caretStart?: boolean): void; // 先 endEdit 旧的；调用 Rike.setEditing
export function endEdit(): void;                                    // 把草稿 flush 进 Rust（setDraft）后 Rike.clearEditing，editing 置空
export function noteDraft(rel: string, id: string, text: string): void; // 记下最新草稿，200 ms 定时 flush；不触发任何重绘
export function flushDraft(): string | undefined;                   // 立即 setDraft 并返回草稿文本（没有草稿返回 undefined）；结构操作前调用
export const focusRequest: () => { id: string; caretStart: boolean } | null; // 结构操作后 Rust 要求聚焦的块
export function clearFocusRequest(): void;

// 结构操作：f 用当前页 generation 调 Rike.xxx 并返回 OpResultDto；store 用结果替换缓存的页面，
// 把 result.focus 设为 focusRequest，并在需要时 startEdit。出错（如 generation 过期）转成 toast。
export function runOp(rel: string, f: (generation: number) => OpResultDto): void;
export function undo(rel: string): void;                            // 编辑中不执行
export function redo(rel: string): void;
export function setTaskDone(rel: string, id: string, done: boolean): void;
export function toggleCollapse(rel: string, id: string): void;
export function acceptDisk(rel: string): void;                      // 先 endEdit，再 Rike.acceptDisk，替换缓存页面
export function keepMine(rel: string): void;

export const backlinksOf: (rel: string) => () => BacklinkGroupDto[];  // 依赖 indexVer
export function openBlock(id: string): void;                        // Rike.block(id) → navigate 到所在页
```

## 验收（每个 worker）

- `cd ui && nsh check` 通过（用 `CARGO_TARGET_DIR=$HOME/.cache/rike-ui-target CARGO_NET_OFFLINE=true TMPDIR=$HOME/.cache/sx-tmp`）。
- `nsh test`（同样环境变量）通过。
- 不改 `crates/`；需要 Rust 侧改动时写进回报，由主 agent 处理。
