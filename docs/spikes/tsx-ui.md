# TSX UI 探针（Input / 列表 / 性能）

> 注：实验工程 `spikes/tsx-ui/` 已在迁移完成后删除；结论保留在这里，对应行为现在由 `ui/*.test.tsx` 覆盖。

NSH rev `a5036af`，纯 TSX 工程 `spikes/tsx-ui/`（`nsh init --template ts`，无 Rust crate，所以 `nsh test` 很快）。
结论表：

| 探针                               | 结论 | 证据 / 命令                             |
| ---------------------------------- | ---- | --------------------------------------- |
| I0 `nsh test` 默认平台             | 待测 |                                         |
| I1 草稿模式与 keyed 列表重取       | 通过 | `spikes/tsx-ui/I1.test.tsx`（2 passed） |
| I2 Input 的 props / 事件           | 待测 |                                         |
| I3 keymap 与输入框焦点、组合期卸载 | 待测 |                                         |
| I4 BackHandler 与 Linux 后退事件   | 待测 |                                         |
| I5 TextView + renderMath 时机      | 待测 |                                         |
| I6 300 行性能                      | 待测 |                                         |

## I1 草稿模式 + keyed 列表（通过）

工程：`spikes/tsx-ui/I1App.tsx` + `I1.test.tsx`，命令 `cd spikes/tsx-ui && nsh test I1.test.tsx`。

结构：`rows` 是 `signal<RowItem[]>`，`<Each of={rows()} key={(r) => r.id}>` 里每行 `<RowView row={...}>`，行内 `<Input defaultValue={row().text} onChange={..}>` 写进非响应式普通变量 `draft`（对应 Rust 的 `Ctx.draft`）；行首 `Text` 打出 `[id:m<n>]`，`n` 是这个组件函数体里 `++mountSeq` 的结果，用来数挂载次数。

结论：

1. **同 id、同 rev 的新对象整体替换数组（`rows.set(rows().map(r => ({...r})))`）：行不重建。** 挂载标记仍是 `[1:m1]` / `[2:m2]`；已输入的 `firsthello` 在 `Input` 里还在，焦点也还在（直接 `app.keyboard.press("x")` 变成 `firsthellox`，不用重新点输入框）；`draft` 镜像收到 `{"id":"1","text":"firsthellox"}`。证据：测试 1 通过。
1. **rev 变了（同一 id）也不重建行，只有绑定的那部分更新。** `rows.set(...rev:"r2", text:"server-1")` 之后 `TextView` 显示 `server-1`，但挂载标记还是 `[1:m1]`，`Input` 里的 `firsthellox` 保留。也就是说组件函数**没有被重新调用**（`mountSeq` 不涨），更新是逐节点 patch；`Each` 的 key 只有 id，rev 不参与。
1. **keyset 变化不重建其它行。** 顶部插入新行 `[0:m3]` 后，`[1:m1]`、`[2:m2]` 不动，`in-2` 的 `secondtyped` 保留。
1. **删除再加回 = 新行、新挂载、`defaultValue` 重新读一次。** `Remove 1` 后 `in-1` 消失，`Add back 1` 得到 `[1:m4]`，值回到 `first`（打过的字丢了）。这是 keyed 列表的正常语义。

对 Rike 的含义：

- 用 `defaultValue` + 非响应式草稿变量做“编辑不重绘”，在 `Each` + key=块 uuid 下成立：reload / 重取 / 相邻块增删都不会把正在编辑的 `Input` 卸载，也不需要重新 `focus()`。
- 但**行被真正移除（块被删、页面切走、列表 key 变了）就一定会重挂载，`defaultValue` 重新读**。所以草稿必须在结构操作前提交进模型（与原 Rust 版“结构操作前先取实时文本”一致），不能指望 `Input` 里留存。
- 异步加载槽（Rust 的 `Signal<Option<Doc>>`）换成 `signal<Doc|null>` 时，`null → Doc` 这一步整个列表区域会重建，正在编辑的块必然重挂载。

**坑（迁移时必须改）**：直接解构 props（`const RowView = ({ row }) => ...`）会让 `nsh` 打警告——

```
warn I1App.tsx:40:18: RowView takes its props apart (`{ row }`): each is read once, when RowView runs, and does not follow changes; take `props` and read `props.row` where it is used (`split(props, [...])` to pass the rest on)
```

传信号访问器（`row: () => RowItem`）时 `row()` 仍然照常追踪，所以本例结果有效；但写成普通值 props 会冻结在第一次的值。迁移时统一用 `props` 形式，或显式传 `() => T`。
