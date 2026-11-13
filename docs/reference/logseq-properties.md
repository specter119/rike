# `key:: value` 属性：兼容面与成本

写这份是因为 `title::` 的讨论牵出一个更大的问题：rike 到底兼容 Logseq 的
`key:: value` 到什么程度，剩下的兼容成本有多大。结论先放这里：

> **解析和保留已经做完，语义只做了两个键，展示和编辑完全没做。**
> 兼容的大头不是解析器，是"属性在界面上不存在"。

## 0. 第一性：它是 metadata，但不是 Markdown 的 metadata

**CommonMark 没有元数据通道。** 所谓"Markdown 的 metadata"全部是扩展，分三类：

| 家族 | 例子 | 分隔 | 作用域 |
| --- | --- | --- | --- |
| frontmatter | Jekyll/Hugo/Obsidian 的 `---` 块 | 有（`---`） | 整篇文档 |
| 节点属性 | Pandoc `[x]{.c}`、kramdown `{: .c}` | 有（花括号） | 单个节点 |
| **位置约定** | **Logseq 的 `key:: value`** | **无** | **靠位置推断** |

`key:: value` 属于第三类，它有三个与"metadata"直觉不符的性质：

1. **没有分隔符**：不知道哪里结束，只能靠结构规则（"第一个 bullet 之前"）
2. **和正文同形**：`note:: hi` 在普通 Markdown 里就是一段话
3. **作用域靠位置**：块属性属于"它是谁的续行"，所以行序决定语义

第 3 条是这个格式的固有代价：**移动一行文本就可能把属性降级成正文**。这不是兼容
问题，纯 rike 库里也存在。rike 用两个设计把这个代价关掉了：序列化时属性先写、正文
续行后写（`serialize.rs:47`），所以正文里长得像属性的行不会被误读回来；加上严格语法
和往返自检。

### 身份不是 metadata

`title::` 那场争论的真正错误不是"哪个优先"，而是**把 metadata 当成了身份**。

- **身份（address）**：这份文档在哪。文件名就是地址。
- **元数据（description）**：这份文档说自己是什么。`title::`、`tags::`、`created-at::`。

Logseq 把标题（元数据）当地址，靠"标题 → 文件名"的编码把两层绑起来。编码无损时
绑定成立；编码有损时（`.` 被当成 namespace）绑定会断，于是需要 `title::` 去修补。
rike 的编码无损，所以可以直接用文件名当地址，元数据退回元数据——这才是 P1 的依据。

### 所以正确的模型是

```
文档 = 元数据 map + 正文
```

而不是"正文里混着元数据行"。rike 的数据模型已经是这个形状（`Page.items` 的
`PageProperties`、`Block.properties`），缺的是**把它当 metadata 对待**：展示、类型、
查询、编辑。

## 1. 它不是 Markdown，也不是 org

| 语法 | 出处 |
| --- | --- |
| `# 标题`、`- 列表`、`[文本](url)`、代码围栏 | CommonMark |
| `---` 包起来的 YAML frontmatter | Jekyll/Pandoc 约定 |
| `:PROPERTIES:` 抽屉、`#+TITLE: value` | org-mode |
| **`key:: value`** | **Logseq 自己的语法** |
| `[[双链]]`、`#tag`、`((uuid))`、`![[嵌入]]` | Obsidian / Logseq 方言 |

所以属性不是"Markdown 的一个特性"，是 Logseq 在 Markdown 上叠的一层。用普通
Markdown 渲染器打开，`title:: Bar` 就是一段普通文字。

## 2. 位置规则

| 位置 | 规则 | 代码 |
| --- | --- | --- |
| 页面属性 | 文件顶部、**第一个顶层 bullet 之前**、列 0 | `crates/rike-core/src/parse.rs:199` |
| 块属性 | bullet 首行之后**紧跟的续行**（`缩进 + 两个空格`），遇到第一个非属性续行或空行就结束 | `crates/rike-core/src/parse.rs:288` |
| 其他位置 | 当正文，原样保留 | 同上 |

细节：

- 页面属性可以在 raw 段落之后，只要还在第一个 bullet 之前。
- 块首行本身永远是正文：`- title:: x` 是文本，不是属性。
- 块内出现空行就结束属性阶段，后面的 `key:: value` 是正文。
- 代码围栏里的 `key:: value` 永远是正文。

## 3. 接受语法（比 Logseq 严格）

`parse_property`（`crates/rike-core/src/parse.rs:112`）：

| 写法 | rike | 理由 |
| --- | --- | --- |
| `key::` | 属性，空值 | 无歧义 |
| `key:: value` | 属性 | 一个空格 + 非空值 |
| `key::value` | **正文** | 不确定是不是属性就不动字节 |
| `key::  value`（两个空格） | **正文** | 同上 |
| `a b:: v` | **正文** | key 不能含空白 |
| `a:b:: v` | **正文** | key 不能含 `:` |

严格是"无损兜底"的直接结果：只要有一个字节不确定，就当正文保留，保证
`serialize(parse(x))` 不丢东西。

## 4. key 规范化

```rust
// crates/rike-core/src/model.rs:46
pub fn key(&self) -> String {
    self.raw_key.to_lowercase().replace('_', "-")
}
```

- 查找/索引用规范化后的键（`created_at::` 和 `created-at::` 是同一个键）
- 写回用 `raw_key` 原样（`crates/rike-core/src/serialize.rs:60`），不改用户字节

## 5. rike 现在怎么用它

| 键 | rike 的语义 | rike 会写吗 |
| --- | --- | --- |
| `id::` | 块身份，`((uuid))` 按它解析 | 块被引用时写 |
| `collapsed::` | 折叠状态 | 折叠/展开时写 |
| `title::` | **无**（2026-11-13 起不参与身份，不一致只发通知） | 从不 |
| 其他 | 不透明，原样保留 | 从不 |

另外两条容易漏的行为：

- **块属性值会被扫描引用**：`referenced_keys`（`crates/rike-index/src/mem.rs:129`）
  对除 `id`/`collapsed` 之外的每个属性值扫 `[[x]]` 和 `#x`；`tags::` 额外按逗号切分。
  所以 `tags:: a, b` 和 `tags:: [[a]], [[b]]` 都会产生反链。
- **页面属性不参与索引**：`page_properties()` 只在 `resolve_title` 里用过（现已删除），
  索引从不扫页面级属性。所以**页面级 `tags::` 不产生反链，块级会**——这个不对称是
  真实的兼容缺口。

## 6. 影响评估

### 场景 A：读别人的 Logseq 库

| 方面 | 影响 |
| --- | --- |
| 字节 | 不丢。不认识的属性原样保留，往返自检兜底 |
| 反链 | 块级 `tags::` 正常；**页面级 `tags::` 丢失** |
| 链接 | `alias::` 不解析（`[[别名]]` 会新建空页） |
| 显示 | 属性在界面上完全不可见 |
| 编辑 | 属性不属于块正文，编辑器里改不到 |

结论：**能打开、能读、不损坏，但会静默少掉一些语义**。最难受的是"看不见"——
用户不知道自己的 `tags::`/`alias::` 在 rike 里没生效。

### 场景 B：当日常主力用

属性是 Logseq 用户的日常工具（`tags::`、`alias::`、`created-at::`、
`background-color::`、`card-*` 间隔重复、`query::`、`template::`、`public::`、
`exclude-from-graph-view::`、`icon::`）。rike 一个都不支持，而且看不到。

结论：**这是"能不能当主力"级别的缺口**，不是细节。

## 7. 成本

| 级别 | 内容 | 具体工作 | 相对成本 |
| --- | --- | --- | --- |
| L1 解析 + 保留 | 已有 | — | 0 |
| L2 只读展示 | 页面卡片和块行显示属性 | DTO 加字段（`PageDto`/`BlockDto`）、`doc.tsx`/`block.tsx` 渲染、两个 UI 测试 | **小** |
| L3 可编辑 | 属性能改 | 属性编辑控件、保存路径（属性不属于 `Input` 的文本，要单独提交）、撤销、冲突/stale 处理、往返测试 | **中**，且会碰 `write-safety` 的每一条 |
| L4 语义键 | 每个键一个功能 | `alias::`（依赖 `rel_for_title -> Option`）、`created-at::`/`updated-at::`、`background-color::`、`card-*`、`query::`、`template::`、`public::` | **大**，按需，一个键一个里程碑 |

页面级 `tags::` 进索引是 L1 和 L2 之间的一个独立小项：改 `referenced_keys` 的调用点，
让页面属性也走同一套扫描。

## 8. 兼容风险清单

1. **严格语法的语义丢失**：`key::value`（无空格）在 rike 里是正文。字节不丢，但
   Logseq 若把它当属性，这个属性在 rike 里就"消失"了。Logseq 的精确语法**未验证**
   （AGPL 源码不读，公开文档没有语法级说明）。
2. **页面/块不对称**：页面级 `tags::` 不产生反链。
3. **key 规范化合并**：`a_b::` 和 `a-b::` 在 rike 里是同一个键；如果同一页里两个都写，
   查找只看第一个。
4. **属性位置**：Logseq 是否允许页面属性出现在 raw 段落之后，未验证；rike 允许。
5. **`title::` 已分叉**：见 `docs/reviews/2026-11-13-stance-pass.md#7`——Logseq 用标题
   当身份，rike 用文件名当身份，理由是 rike 的文件名编码无损。

## 9. 建议顺序

1. **L2 只读展示**：把"隐形数据"变成"可见"，成本最小、收益最大，而且不碰写路径。
2. **页面级 `tags::` 进索引**：消除不对称，几行代码加一个测试。
3. **L3 可编辑**：要过一遍 stance review，因为它直接改用户字节。
4. **`alias::`**：等 P2 的 `rel_for_title -> Option` 落地，否则会在索引未就绪时复活
   S-A（见 pass 记录 §9.2）。

## 10. 官方文档对照（2026-11-13）

来源：本仓库的 Logseq 官方文档快照 `docs/reference/logseq/`（由 `scripts/sync-logseq-docs.sh`
从 `logseq/docs` 拉取，MIT；**只看文档不看源码**）。

| 页面 | 用途 |
| --- | --- |
| `logseq/pages/Markdown.md` | 标准语法 + 扩展语法清单 |
| `logseq/pages/Properties.md` | 属性命名规则、页面/块属性、值里的链接、逗号分隔、引号 |
| `logseq/pages/Built-in Properties.md` | 内建属性清单（可编辑 / 隐藏） |
| `logseq/pages/Filename format.md` | `:legacy` vs `:triple-lowbar` 编码 |
| `logseq/pages/Logseq file and folder naming rules.md` | 文件名字符限制 |
| `logseq/pages/Org Mode.org` | org 格式对照 |

刷新：`scripts/sync-logseq-docs.sh`；检查上游漂移：`scripts/sync-logseq-docs.sh --check`。

### 10.1 `title::` 是官方定义的「覆盖文件名」

> `title` (1) (page) - **overrides the title of a page to be different from the file name.**

所以 P1（身份只来自文件名）**与官方行为冲突**，已回滚。`title::` 恢复为身份来源。

### 10.2 文件名格式：rike 的编码就是 `:triple-lowbar`

> `:legacy`
> - Use Percent-encoding for `/` and other invalid characters
> - 早期版本用 `.` 编码 `/`
> - **Add `title::` properties to pages with encoded characters to avoid ambiguity**
>
> `:triple-lowbar`（0.8.9 起，新库默认）
> - Use triple underscore `___` for slash `/`
> - Use Percent-encoding for other invalid characters
> - **No more `title::` property required**（仍可手动使用）

rike 的 `___` + 百分号编码 = `:triple-lowbar`。这条解释了为什么 rike 自己从不写 `title::`：
在新格式里它不是必需的。但**「不必需」不等于「不用」**——用户手动写的时候，Logseq 会认。

配置在 `logseq/config.edn` 的 `:file/name-format`，rike 已经读它并在非 `:triple-lowbar` 时告警。

**2026-11-13 决定：只支持 `:triple-lowbar` 库。** `:legacy` 库收到告警，不在兼容范围内。
因此 S-A（catalog 看不见 `title::`）的剩余风险被缩小到“`:triple-lowbar` 库里用户手动覆盖了
`title::` 的页”，属于已文档化的限制，不再阻塞。

### 10.3 属性命名规则：rike 比官方宽

官方：

> Property name begins with a non-numeric character and can contain alphanumeric
> characters and `. * + ! - _ ? $ % & = < >`. If `-`, `+` or `.` are the first
> character, the second character (if any) must be non-numeric.
>
> Properties are case insensitive and lower cased. Properties with a `_` character
> are renamed to `-`.

rike：key 非空、不含空白、不含 `:`，其余任意。所以 rike 接受 `@x::`、`#x::` 这类官方会拒绝的键。
宽松对"不丢字节"是安全的，但**写回时不会规范化**，和 Logseq 的写法可能不一致。

### 10.4 属性值里的链接：官方会建链，rike 只扫块级

> Property values can be a mix of almost any text, links, page links and tags.
> The pages `Logseq,`, `triples` and `text editor` are all linked property values
> through the `description` property.

rike 只扫**块属性**值（`mem.rs#referenced_keys`），**页面属性不参与索引**。这是明确的兼容缺口。

另外两条官方规则 rike 还没实现：

- **逗号分隔**：`tags:: motor, steering wheel` 里 `motor` 和 `steering wheel` 都是页面引用。
  内建对 `tags`/`alias` 生效，其余属性靠 `:property/separated-by-commas` 配置。
  rike 只对块级 `tags::` 做了逗号切分。
- **引号抑制链接**：`description:: "[[Logseq]] ..."` 里不建链。rike 没有这个规则。
- **属性值不能换行**（官方明确），rike 的解析规则天然满足。

### 10.5 内建属性清单

| 类别 | 键 |
| --- | --- |
| 可编辑（页面） | `icon`、`title`、`filters`、`public`、`exclude-from-graph-view` |
| 可编辑（通用） | `tags`、`template`、`template-including-parent`、`alias` |
| 隐藏（块） | `collapsed`、`id`、`created-at`、`updated-at`、`query-table`、`query-properties`、`query-sort-by`、`query-sort-desc` |

rike 有语义的只有 `id` 和 `collapsed`（读写），`title`（读）。

### 10.6 扩展语法清单（`Markdown.md`）

| 语法 | rike |
| --- | --- |
| `[[page name]]` | ✅ |
| `#tag`、`#[[page name]]` | ✅ |
| `((block-uuid))` | ✅ |
| `property:: value` | ✅ |
| `[display text]([[page name]])` | ❌ 带标签的页面引用 |
| `[display text](((block-uuid)))` | ❌ 带标签的块引用 |
| `{{embed [[page]]}}`、`{{embed ((uuid))}}` | ❌ |
| `TODO` `DOING` `DONE` `LATER` `NOW` | 部分（rike 只认 TODO/DOING/DONE） |
| `[#A]` `[#B]` `[#C]` 优先级 | ❌ |
| `^^highlight^^` | ❌ |
| 图片 `{:height 53, :width 42}` | ❌ |
| Hiccup `[:p "Hello"]` | ❌（M1 不做） |

### 10.7 兼容清单

| 项 | 状态 |
| --- | --- |
| `:triple-lowbar` 文件名编码 | ✅ |
| `title::` 覆盖文件名 | ✅（回滚后） |
| 属性命名规范化（小写、`_`→`-`） | ✅ |
| 属性往返不丢字节 | ✅ |
| 页面属性值建链 | ❌ |
| 逗号分隔（`tags`/`alias`） | 仅块级 `tags` |
| 引号抑制链接 | ❌ |
| `alias::` 参与解析 | ❌（P2/M2） |
| 带标签的引用 | ❌ |
| `{{embed}}`、优先级、高亮、图片属性 | ❌ |
