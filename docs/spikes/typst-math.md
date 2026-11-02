# 实验：用 Typst 渲染 LaTeX 公式

目录 `spikes/typst-math`。结论：**可行，已接入 `rike-native`**（`crates/rike-native/src/math.rs`）。下面是接入前的实验记录，渲染代码已搬进原生模块，这个目录只剩命令行和公式提取器。

## 要解决的问题

NSH 的 `TextView` 已经会解析 `$…$`、`$$…$$`、`\(…\)`、`\[…\]`，并区分行内和展示公式；但没设 `render_math` 回调时只把 TeX 当代码显示。缺的只是“TeX → 图片”这一步。

NSH 对两种写法的规则（`nsh-base/src/text_view/markdown/latex.rs`）：

- 展示公式（回调的 `display = true`）：`\[` 在块的行首、`\]` 在同一行或之后的行末（中间不能有空行），改写成 `$$` 围栏；或者 `$$…$$` 独占一行。
- 行内公式（`display = false`）：`\(…\)`，以及夹在文字中间的 `\[x\]`。

## 做法

```
TeX ──mitex──▶ Typst 数学 ──typst 编译──▶ 页面 ──typst-render──▶ PNG ──data: URI──▶ NSH <Image>
```

- 行内公式生成 `$x$`，展示公式生成 `$ x $`，保留两者区别。
- 页面 `width: auto, height: auto, margin: 2pt, fill: none`，输出透明 PNG，文字颜色由调用方传入（深色主题要传浅色）。
- 字体用 `typst-assets` 内嵌的（含 New Computer Modern Math），不读系统字体。
- 选 PNG 不选 SVG：没有确认 NSH 的 SVG 渲染器支持 Typst 输出里的 `<use>`/`<symbol>`。NSH 能直接加载 `data:` 位图（`nsh-core/src/image/loader.rs`）。

## 结果

用 `typst-math scan <dir>` 对真实笔记库（一个真实笔记库，只读）里的 102 个公式（行内和展示都有）做了转换和编译：

- 第一轮：71 成功、31 失败，全是 `mitex` 输出里 Typst 0.15 没有的名字。
- 补完兼容定义后：**102/102 成功**。
- 渲染耗时（release，单个公式，含布局和光栅化）：中位数约 1 ms，p95 约 7 ms，最大 13 ms。
- 肉眼检查了分式、偏导、`\left…\right`、`cases`（`array`）、`aligned`、`\operatorname`、求和下标、`\mathbf`，排版正确。

补的兼容定义在 `crates/rike-native/src/math.rs` 的 `PRELUDE` 和 `RENAMED`：`mitex` 面向旧版 Typst，它的输出会用到 `diff`（现为 `sym.partial`）、`dot.circle`（现已改名）、`mitexmathbf`、`mitexsqrt`、`operatorname`、`aligned`、`mitexarray`、`varPhi` 等，这些要自己定义。新公式遇到新缺口时，往这两处加。

## 代价

1. **体积和编译时间**：Typst 全家加内嵌字体。实验程序 release 去符号后 46 MiB，其中字体约 9.6 MiB；首次 release 编译约 2 分钟（28 分钟 CPU）。没有量过 Rike 本身 release 的基线，所以增量未知。
1. **转换覆盖不全**：`mitex` 对没见过的命令会失败，所以回调里要保留回退：编译失败就显示原来的 TeX 源码。已知的不精确处：`\sqrt[n]{x}` 的 n 会带方括号；`array` 的列对齐（`{ll}`、`{c|c}`）被忽略；`\|\boldsymbol{X}\|` 的范数符号后有多余空格。
1. **许可证**：`typst*`、`mitex` 都是 Apache-2.0，可以用。不要用 `tex2typst-rs`（GPL-3.0）。

## 接入结果

- `ui/src/math.tsx` 的 `renderMath` 是 `TextView`（块和反链）的 `renderMath` 回调。行内公式按基线对齐，展示公式居中，宽公式用 `maxWidth: "100%"` + `aspectRatio` 缩小。
- 基线用 7 pt 透明标记读取，文字高度用 `top-edge/bottom-edge: "bounds"`，细节见 `AGENTS.md`“公式渲染”。
- 缓存含失败项，有字节上限；颜色随深浅主题；启动时在后台线程预热。
- 失败回退为等宽 TeX。点公式进入块的源码编辑。
- 体积：接入后 `rike` release 去符号约 80 MiB（83.6 MB）。没有量过接入前的基线；实验程序里 Typst 加字体是 46 MiB。
- 在无头 sway 里用 `scripts/wayland-ime-smoke.sh math` 截图确认了显示（行内、展示、回退）。HiDPI 缩放没有验证。

## 复现

```
cargo build -p typst-math --release
target/release/typst-math one '\frac{a}{b}' --display out.png
target/release/typst-math scan <vault 目录>   # 用的是 rike_native::math
cargo test -p typst-math
```
