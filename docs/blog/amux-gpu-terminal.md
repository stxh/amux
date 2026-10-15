# amux：把 Ghostty 的终端引擎，装进 GPU 渲染的界面里

> 一个用 Rust + Xilem/Vello + libghostty-vt 写的跨平台终端复用器。
> tmux 的快捷键肌肉记忆，现代 GUI 的渲染与交互。

## 你是不是也这样

- tmux 的前缀键已经刻进手指，但它的界面停留在上个时代；
- Windows Terminal / iTerm2 足够好看，可分屏、会话、工作区的组合总差一点；
- 喜欢 macOS 上 cmux 的体验，人却在 Windows / Linux 上搬砖。

`amux` 想把这些缝起来：**tmux 式的复用逻辑 + Ghostty 的终端引擎 + Vello 的 GPU 渲染**，一套代码跑在 Windows / macOS / Linux。

## 它是什么

| 层 | 选型 | 负责 |
| :-- | :-- | :-- |
| 终端引擎 | `libghostty-vt` | ANSI 解析、滚动回滚、选区、256 色 / 真彩 |
| 渲染与 UI | `Xilem` + `Vello` | 声明式布局、窗口、GPU 合成 |
| 进程 | `portable-pty` | 每个面板一个真实 shell |

界面不是"终端里嵌 UI"，而是**整块终端网格由 canvas 直接绘制**，活动栏、Tab 栏、滚动条、设置面板都是框架里的普通控件。

## 值得一试的四个点

### 1. 现代终端该有的样子

- 左侧活动栏：workspace 图标 + 第一个位置的齿轮（系统设置入口）；
- 圆角卡片式 Tab，自动跟随 shell 标题；
- 只在有回滚时才出现的半透明滚动条；
- 模态设置面板：字号 ±、左右边栏、单实例模式、打开配置目录、退出。

### 2. 交互抠到终端老炮的水平

- **滚动条能拖**：点轨道翻页，按住滑块拖动；
- **拖出窗口也不丢**：拖到窗口外继续跟随，松手正常收尾（masonry 指针捕获 + Win32 `SetCapture`）；
- **选区越界自动滚动**：把选择拖到面板 / 窗口外会持续朝该方向滚动，且**起点固定、选区持续扩展**——这是把两端分别锚定在"内容坐标"和"可见坐标"上才做得到的；
- 双击选词、三击选行、右键粘贴、选中即复制、关闭按键 Echo 的快捷键拦截；
- `Ctrl+B` 前缀模式 + 全套 `Ctrl` 组合键，两种肌肉记忆都照顾。

### 3. 性能：滚动卡顿的锅不在 GPU

vello 一行代码都没改，卡顿来自 CPU 侧：

- **每个字符每帧跑一次完整排版**。80×24 一屏约 2000 个单元格，原来每格都走一遍 parley 的 bidi → itemize → 字体 fallback → shaping，单帧 20–60ms。现在按「字号 + 文本」缓存 layout，单元格度量（`cell_w/cell_h`）也一并缓存，查找零分配；
- **滚动条拖动不再每帧问引擎**。`scrollbar()` 在 viewport 被 pin 时要去遍历 page list，现在按下时快照一次基线，拖动过程只做加减；
- 想自己验证？`AMUX_FRAME_LOG=1 amux` 会每 120 帧打印一次场景录制的 avg / max 耗时。

### 4. 跨平台的现实做法

Ghostty 引擎是 `!Send` 的，所以：UI 线程用 `RefCell` 独占引擎，PTY 读线程只把字节块推进 channel，再由 UI 线程 `vt_write`；需要跨线程唤醒时走 xilem 的 `MessageProxy`，整条链路不用轮询。Windows 上还有单实例支持——第二次 `amux <目录>` 会把路径转给已运行实例并直接开新 Tab。

## 架构速览

```
+--------------------------------------------------+
| 活动栏 |  Tab 栏                                  |
|        +---------------------------------------+ |
|        | Xilem/Vello canvas（终端网格直绘）      | |
|        |  pane tree：嵌套横竖分屏                 | |
|        |  每格 = libghostty-vt 渲染状态快照       | |
|        +---------------------------------------+ |
+--------------------------------------------------+
 PTY 读线程 ──channel──> UI 线程 ──RefCell──> Ghostty 引擎
                            ^
                     MessageProxy 唤醒（无轮询）
```

## 快捷键

| 快捷键 | 作用 |
| :-- | :-- |
| `Ctrl+B` 前缀 | 之后按 `c`/`d`/`%`/`"` 分屏，`n`/`p` 切面板，`x` 关面板，`t` 新 Tab，`w` 关 Tab，`b` 侧栏 |
| `Ctrl+Shift+D` / `Ctrl+D` | 上下 / 左右分屏 |
| `Ctrl+T`、`Ctrl+W` | 新 Tab / 关 Tab |
| `Ctrl+Shift+[` / `]` | 上一个 / 下一个 Tab |
| `Ctrl+N`、`Ctrl+Shift+R` | 新 workspace / 重命名 |
| `Ctrl+Alt+方向键` | 按方向聚焦面板 |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | 字号放大 / 缩小 / 复位 |
| `Ctrl+,`、`Ctrl+Shift+,` | 打开设置 / 打开配置目录 |
| `Shift+PgUp` / `PgDn` | 滚动回滚 |

## 跑起来

```bash
git clone https://github.com/stxh/amux.git
cd amux
cargo run                 # 或 cargo build --release
```

首次构建 `libghostty-vt-sys` 会拉取固定的 Ghostty commit 并用 Zig 0.15.2 编译（需要网络）；离线 / 可复现构建可以把 `GHOSTTY_SOURCE_DIR` 指到本地 Ghostty 源码检出。

## 路线图

- 会话保存与恢复、分割线拖动调比例
- 主题与字体配置（设置面板里先放最常用的）
- Linux / macOS 交互细节打磨、IME 深度适配
- 大 scrollback 下的渲染进一步分层

## 最后

如果你在找一个**能看见每一帧在干什么**的终端复用器，或者想看看 `Xilem` + `Vello` + 成熟终端引擎拼在一起是什么效果，欢迎来 [github.com/stxh/amux](https://github.com/stxh/amux) 试试：issue、PR、或者只是说说你想要的快捷键，都欢迎。
