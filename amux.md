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