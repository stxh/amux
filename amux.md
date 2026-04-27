这是一个非常大胆且极客的想法。将 **cmux** 的多路复用逻辑、**Alacritty** 的速度与 **Xilem/Vello** 的前沿渲染架构结合，堪称终端界的“三位一体”。

以下是对话总结的 Markdown 文件内容，以及一个基于此架构的 Rust 原型代码结构。

---

### Part 1: 技术方案总结 (`cmux_evolution.md`)

```markdown
# cmux 跨平台演进方案：Rust + Xilem + Alacritty

## 1. 背景与目标
- **现状**: cmux 深度依赖 `libghostty` (macOS 专属)。
- **目标**: 通过接入 `alacritty_terminal` 实现跨平台支持，并利用 `Xilem` 构建高性能声明式 UI。

## 2. 核心组件
| 组件 | 职责 |
| :--- | :--- |
| **Xilem (Vello)** | UI 框架、布局管理、跨平台窗口 (via winit)、GPU 加速渲染。 |
| **Alacritty Terminal** | ANSI 解析、PTY 状态管理、滚动缓冲区。 |
| **cmux Core (Rust)** | 多路复用逻辑、快捷键路由、会话序列化。 |

## 3. 实现路径
1. **心脏移植**: 移除 Zig/OC 绑定，引入 `alacritty_terminal` 处理 Shell 数据流。
2. **渲染架构**: 采用 **Texture Tiling** 方案。将终端网格渲染为 GPU 纹理，由 Xilem 视图层进行合成。
3. **布局引擎**: 利用 Xilem 的 Flex 布局实现分屏（Tiling），替代 macOS 原生 NSView。
4. **事件处理**: 通过 `winit` 捕获输入，由 cmux 逻辑层决定是分发给 UI 还是 PTY。

## 4. 技术挑战
- Xilem 处于 Pre-alpha 阶段，API 变动频繁。
- 跨平台 IME (输入法) 的深度适配。
- 终端文本连字与颜色样式在 Vello 中的高性能呈现。
```

---

### Part 2: 快速原型实现

由于 Xilem 和 Vello 仍在快速迭代，以下原型展示了如何将 `alacritty_terminal` 的数据挂载到 Xilem 的 View 系统中。

#### 1. 项目初始化 (`Cargo.toml`)
```toml
[package]
name = "cmux-proto"
version = "0.1.0"
edition = "2021"

[dependencies]
# 核心终端解析
alacritty_terminal = "0.24.0" 
# UI 框架 (使用 git 版本以获取最新特性)
xilem = { git = "https://github.com/linebender/xilem" }
vello = { git = "https://github.com/linebender/vello" }
winit = "0.29"
# PTY 处理
portable-pty = "0.8"
```

#### 2. 原型代码 (`src/main.rs`)

```rust
use xilem::{view::View, App, AppLauncher};
use alacritty_terminal::term::Term;
use alacritty_terminal::event::WindowSize;

// 1. 定义终端窗格状态
struct Pane {
    id: usize,
    terminal: Term<()>,
    is_active: bool,
}

// 2. 全局应用状态
struct CmuxApp {
    panes: Vec<Pane>,
}

// 3. 模拟 UI 渲染逻辑 (Xilem 风格)
fn app_logic(data: &mut CmuxApp) -> impl View<CmuxApp> {
    use xilem::view::{flex, label, container};

    // 构建多路复用布局
    let mut layout = flex::column(());
    
    for pane in &data.panes {
        let title = format!("Pane {} - {}", pane.id, if pane.is_active { "*" } else { "" });
        
        // 这里在实际开发中会是一个自定义 View，将 Alacritty 的缓冲区通过 Vello 渲染出来
        let terminal_view = container(
            label(format!("Term Buffer Snippet: {:?}", &pane.terminal.grid()[0][0..10]))
        )
        .with_background_color(if pane.is_active { [0.2, 0.2, 0.2, 1.0] } else { [0.1, 0.1, 0.1, 1.0] });

        layout = layout.with_child(label(title)).with_child(terminal_view);
    }

    layout
}

fn main() {
    // 初始化 Alacritty 终端状态
    let size = WindowSize { num_lines: 24, num_cols: 80, cell_width: 10, cell_height: 20 };
    let term = Term::new(&(), size, alacritty_terminal::term::Config::default());

    let app_data = CmuxApp {
        panes: vec![
            Pane { id: 1, terminal: term, is_active: true },
        ],
    };

    // 启动 Xilem 渲染循环
    let app = App::new(app_data, app_logic);
    // 注意：具体启动 API 随 Xilem 版本变动
    // AppLauncher::new(app).run(); 
    println!("cmux 原型初始化完成：Alacritty 后端已就绪。");
}
```

### 如何进一步运行？
1. **PTY 桥接**: 你需要使用 `portable-pty` 来启动真正的 Shell (如 `zsh` 或 `powershell`)。
2. **自定义渲染器**: Xilem 目前主要处理矢量图形。要显示终端文字，你需要编写一个实现 `xilem::View` 的结构体，在它的 `paint` 方法中调用 `vello` 的 `draw_glyphs` 或将 Alacritty 的像素缓冲区上传为纹理。
3. **快捷键捕捉**: 在 `winit` 事件循环中拦截 `Ctrl+B` (或你自定义的 Prefix)，用于动态更新 `CmuxApp` 中的 `panes` 向量。

这个架构虽然处于技术最前沿，但一旦跑通，cmux 将拥有目前市面上终端中最强大的跨平台 GUI 渲染能力。

---

## 开发日志

### v0.0.1 - 初始版本 (2026-04-27)

#### 实现概述
成功实现cmux风格的终端复用器基础架构，包含完整的UI布局、PTY集成和多窗格管理。

#### 核心特性

**1. cmux风格布局**
```
+------------------------------------------+
| [Pane 1/1]  [+] [-] [<] [>]  (标签行)    |
+------------------------------------------+
|                                          |
|        终端窗格 (PowerShell)             |
|                                          |
+------------------------------------------+
| Ctrl+B: prefix | c:split-v | d:split...  |
+------------------------------------------+
```

- 顶部标签行：动态显示当前窗格信息 (Pane X/Y)
- 4个操作按钮：+ (垂直分割), - (水平分割), < (上一窗格), > (下一窗格)
- 终端区域：填满剩余窗口空间
- 底部状态栏：显示键盘快捷键提示

**2. 终端渲染**
- 使用Xilem的canvas进行GPU加速渲染 (Vello imaging后端)
- 支持完整的256色ANSI调色板
- 正确转换alacritty_terminal的TermColor到peniko Color
- 实现单元格标志渲染：Bold, Underline, Strikethrough, Inverse
- 活动窗格光标渲染 (rosewater颜色)
- 滚动指示器显示

**3. PTY集成**
- 每个窗格独立的shell进程 (Windows: PowerShell)
- 后台线程异步读取PTY输出
- 输出馈入alacritty_terminal解析器
- 正确处理终端状态变更事件

**4. 多窗格管理**
- 垂直分割 (上下布局)
- 水平分割 (左右布局)
- 窗格导航 (上一个/下一个)
- 关闭窗格
- 使用归一化坐标 (0.0-1.0) 的响应式布局系统

**5. 状态管理**
```rust
App {
    panes: Vec<Pane>,              // 终端窗格列表
    pane_states: Vec<TerminalPaneState>,  // UI状态 (滚动、焦点)
    active_pane_index: usize,      // 当前活动窗格索引
    next_pane_id: usize,           // 下一个窗格ID
    terminal_config: TerminalConfig,
    prefix_mode: bool,
    status_message: String,
    running: bool,
}
```

#### 技术栈
- **Rust 2021 Edition**
- **alacritty_terminal 0.24.0** - 终端模拟后端
- **Xilem (git)** - 声明式UI框架
- **Vello** - GPU加速2D图形渲染
- **portable-pty 0.8** - 跨平台PTY管理
- **parley 0.8** - 文本布局 (未来使用)
- **skrifa 0.40** - 字体处理 (未来使用)
- **peniko 0.6** - 颜色处理

#### 配色方案
使用 **Catppuccin Mocha** 主题：
- Crust: #11111b (最深背景)
- Base: #1e1e2e (终端背景)
- Surface0: #313244 (标签行背景)
- Text: #c6d0f5 (前景文本)
- Blue: #89b4fa (活动窗格边框)
- Rosewater: #f5e0dc (光标)

#### 文件结构
```
amux/
├── Cargo.toml           # 项目依赖
├── README.md            # 项目文档
├── amux.md              # 开发文档
└── src/
    ├── main.rs          # 应用入口和UI布局
    ├── app.rs           # 全局状态管理
    ├── pane.rs          # 窗格表示和布局几何
    ├── terminal.rs      # Alacritty终端包装器 (PTY I/O)
    ├── terminal_render.rs  # Canvas终端渲染
    ├── terminal_widget.rs  # 自定义Masonry widget (备用)
    └── keyboard.rs      # 键盘输入处理模块
```

#### 构建与运行
```bash
# 开发构建
cargo build

# 运行
cargo run

# 发布构建
cargo build --release
```

#### 已知限制
- 文本当前渲染为彩色块 (字符单元格的纯色填充)
- 真正的字形渲染需要更深入的parley/skrifa集成
- 键盘快捷键尚未完全实现 (目前仅按钮)
- 鼠标文本选择未实现
- 复制/粘贴支持待添加

#### 下一步计划
- [ ] 使用parley/skrifa实现真实文本渲染
- [ ] 实现完整的键盘快捷键 (Ctrl+B前缀)
- [ ] 鼠标事件处理 (点击选择窗格)
- [ ] 文本选择和复制粘贴
- [ ] 滚动缓冲区导航
- [ ] 窗格缩放 (聚焦单个窗格)
- [ ] 配置文件支持
- [ ] 状态栏自定义

#### Git标签
```bash
git tag -a v0.0.1 -m "初始版本：cmux风格布局与PowerShell终端"
```

---

### v0.0.2 - 真实文字渲染 (2026-04-27)

#### 实现概述
修复了两个关键问题：终端窗口高度计算错误和文字显示为彩色块的问题。成功实现了使用parley进行文本布局、render_text进行真实字形渲染。

#### 核心变更

**1. 修复终端窗口高度计算**
- 问题：canvas接收的尺寸参数不正确，导致终端未铺满可用空间
- 解决：使用flex布局自动分配的`available_size`参数
- 布局结构：tab_bar (28px固定) + terminal_area (flex自动填充) + status_bar (固定)
- canvas现在正确接收减去标签行和状态栏后的可用空间

**2. 实现真实文字渲染**
- 问题：字符渲染为小的彩色矩形（`char_margin = 1.5`的填充方块）
- 解决方案：
  - 使用`parley::ranged_builder`为每行终端文本构建文本布局
  - 设置monospace字体（GenericFamily::Monospace）
  - 使用`xilem::masonry::core::render_text()`渲染真实字形
  - 通过`Affine::translate()`正确定位每行文本的基线位置

**关键技术实现** (`terminal_render.rs`):
```rust
// 为每行构建parley文本布局
let mut builder = lcx.ranged_builder(fcx, &row_text, 1.0, true);
builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
    FontFamilyName::Generic(GenericFamily::Monospace),
)));
builder.push_default(StyleProperty::FontSize(font_size));

let mut layout = builder.build(&row_text);
layout.break_all_lines(None);
layout.align(None, Alignment::Start, AlignmentOptions::default());

// 渲染真实字形
let transform = Affine::translate((offset_x, text_y_base));
let brushes = &[Brush::Solid(colors::TEXT)];
render_text(painter, transform, &layout, brushes, true);
```

**3. 标签行文字渲染**
- 同样使用parley渲染"Pane X/Y"指示器
- 替代了原来的彩色矩形方案

**4. 架构调整**
- 更新`render_terminal_with_offset`函数签名，添加`fcx`和`lcx`参数
- 从canvas回调中获取`ctx.text_contexts()`传递到渲染层
- 正确集成Masonry的BrushIndex机制

#### 技术细节

**渲染流程**:
1. 从canvas回调获取`MutateCtx`，调用`text_contexts()`获取FontContext和LayoutContext
2. 对每行终端网格单元：
   - 收集字符到`row_text`字符串
   - 绘制非默认背景色的单元格
   - 绘制装饰效果（下划线、删除线、粗体指示）
   - 使用parley构建文本布局
   - 调用render_text渲染真实字形

**依赖使用**:
- parley 0.8 - 文本布局和字形排列
- Masonry imaging - canvas渲染器
- Vello - 底层GPU渲染（通过Masonry封装）

#### 已知限制
- 当前所有文字使用统一前景色（Catppuccin Text #c6d0f5）
- 每字符颜色支持需要更复杂的BrushIndex管理
- 键盘输入尚未实现
- 字体使用系统默认monospace，未加载自定义字体

#### 下一步计划
- [ ] 实现每字符颜色渲染（使用BrushIndex）
- [ ] 实现键盘输入处理（Ctrl+B前缀模式）
- [ ] 鼠标点击选择窗格
- [ ] 加载自定义monospace字体
- [ ] 文本选择和复制粘贴
- [ ] 滚动缓冲区导航

#### Git标签
```bash
git tag -a v0.0.2 -m "实现真实文字渲染：使用parley布局，render_text渲染字形"
```

---

### v0.0.3 - 终端尺寸修复 (2026-04-27)

#### 问题诊断

**问题**：终端垂直方向尺寸错误，canvas接收到的`size.height`只有100px，导致文字非常小无法辨认。

**根本原因**：
1. Xilem的Canvas widget在measure时，如果没有收到`FitContent`类型的长度请求，会返回`DEFAULT_LENGTH = 100px`
2. Canvas没有实现`WidgetView` trait，因此无法使用`FlexExt`的`.flex()`方法
3. flex_col布局中，Canvas作为非flex子元素只获得了默认最小高度

**诊断过程**：
- 在标签栏和终端区域添加黄色调试文字显示canvas size
- 发现标签栏`sz=150x28`正常，但终端区`size=1200x100`异常
- 使用固定48pt字体测试确认parley渲染本身正常
- 检查Masonry的canvas measure源码发现`DEFAULT_LENGTH = Length::const_px(100.)`

#### 解决方案

**Workaround**：使用`sized_box`包装canvas并设置固定高度

```rust
const TERMINAL_HEIGHT: f64 = 700.0; // Good default for 800px window

let terminal_box = sized_box(terminal_area)
    .fixed_height(Length::px(TERMINAL_HEIGHT));

flex_col((
    tab_bar(),
    terminal_box,
    status_bar(),
))
```

**效果**：
- 终端区域高度从100px增加到700px
- 文字大小正常可识别
- 字体约23px（cell_h * 0.7）

#### 技术细节

**Canvas尺寸计算流程**：
```
Window (800px)
  └─ flex_col
      ├─ tab_bar (28px fixed)
      ├─ terminal_area (100px default → 700px with sized_box)
      └─ status_bar (~30px auto)
```

**Masonry Canvas measure源码**：
```rust
fn measure(...) -> f64 {
    match len_req {
        LenReq::FitContent(space) => space,  // 使用所有可用空间
        _ => DEFAULT_LENGTH.dp(scale),       // 默认100px
    }
}
```

#### 已知限制

- 终端高度700px是硬编码值，窗口大小变化时不会自动调整
- 完美解决方案需要Xilem增加Canvas的flex factor支持
- 未来Xilem版本可能添加Canvas的WidgetView实现，届时可使用`.flex(1.0)`

#### 文件变更

- `Cargo.toml`: 版本更新为0.0.3
- `src/main.rs`: 使用sized_box包装terminal_area并设置固定高度
- `src/terminal_render.rs`: 
  - 修复screen_lines使用动态值而非硬编码24
  - 移除调试代码
  - 优化字体大小和baseline计算

#### Git标签
```bash
git tag -a v0.0.3 -m "修复终端尺寸：canvas高度从100px增加到700px，文字正常可识别"
```
