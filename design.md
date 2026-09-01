# Never Sleep — macOS UI 设计说明书

菜单栏 Extra + Utility Panel。系统骨架，一点点 accent。不是 iOS 设置页，不是仪表盘网页。

## 1. 窗口模型与激活策略

**选定：Accessory + 菜单栏 Extra。** 不占 Dock。左键打开/聚焦工具面板；右键打开状态项菜单。

与「面板成为活动 App 时出现 App/Edit/View/Window/Help」的冲突：Accessory 应用不接管菜单栏应用菜单，这是系统菜单栏工具的惯例。若改成 Regular 会出现 Dock 图标，气质就偏完整 App。本产品选择让路给系统：完整命令放在 **状态项菜单** 与面板内，不切换 `NSApplicationActivationPolicyRegular`。

| 表面 | 行为 |
| --- | --- |
| 左键状态项 | 显示或隐藏面板（不弹菜单） |
| 右键状态项 | 原生菜单：开始/结束、时长、选项、使用说明、退出 |
| 面板关闭/Escape | 隐藏面板，**不**结束待命 |
| 失焦 | **不**关闭（分离窗口惯例；关屏时仍能点「结束待命」） |
| 菜单栏图标 | 现有 template 太阳/月亮，浅色/深色自动反相 |

面板是带系统交通灯的紧凑工具窗（约 640×420，可有限缩放）。不吸附在图标下。首次打开居中；之后记住位置。不自绘关闭按钮。不 always-on-top（那会抢别的 App）。

## 2. 文字线框

```
┌─ Never Sleep ──────────────────────────────────────────┐
│ ● ● ●                                                  │
├────────────────┬───────────────────────────────────────┤
│ [玻璃/Sidebar] │ [实底详情 · Notes 留白]                 │
│                │                                       │
│ 待命            │  未开启                                │
│  ● 待命     ✓  │  屏幕将关闭，Mac 保持在线                 │
│                │                                       │
│ 选项            │  [ 开始关屏待命 ]                       │
│  ◇ 屏幕        │                                       │
│  ◇ 合盖        │  时长                    无限期 ▾       │
│  ◇ 保护        │                                       │
│  ◇ 通用        │  ⌥⌘P 在关屏时也可用                      │
│                │                                       │
│ 指南            │  详情列宽 ≤ 400pt，居左，不拉满整屏        │
│  ? 使用说明     │                                       │
└────────────────┴───────────────────────────────────────┘
```

侧边栏选中「屏幕 / 合盖 / 保护 / 通用 / 使用说明」时，右侧只显示该主题的短文 + 必要控件，**不要**把所有开关堆在一页。

待命页只保留：状态、主按钮、时长、热键提示。电量/再关屏/合盖等到对应分区。

## 3. 系统控件清单

| 用途 | 控件 |
| --- | --- |
| 菜单栏 | `NSStatusItem`（tray-icon），template image |
| 面板 | 带装饰的 `NSWindow`（tao）；壳用 `NSGlassEffectView` 或 `NSVisualEffectView` Sidebar |
| 导航 | 侧边栏 `NSButton` + SF Symbols；详情 `NSStackView` |
| 主操作 | `NSButton` Push，固有宽度，**不是**全宽 iOS 胶囊 |
| 开关 | `NSSwitch`，行尾 |
| 时长 | `NSPopUpButton` |
| 语言 | `NSSegmentedControl` |
| 使用说明 | `NSScrollView` + 左对齐正文 |
| 退出 | **只在状态项菜单**，面板里不放退出 |

禁止：WebView、自绘皮肤、emoji 当图标、大导航标题、假卡片阴影墙。

## 4. Liquid Glass 与降级

**用玻璃：** 侧边栏、面板壳、临时浮层。

**不用玻璃：** 详情正文、开关说明、使用说明、状态标题。这些用 `windowBackgroundColor` / `controlBackgroundColor`，保证对比。

| 环境 | 侧边栏 | 详情 |
| --- | --- | --- |
| macOS 26+ `NSGlassEffectView` | Regular 玻璃 | 实底 |
| 旧系统 | `NSVisualEffectMaterial::Sidebar` | `windowBackgroundColor` |
| Dark | 同一套语义色；template 符号自动反相 | 实底跟随 window 背景 |
| Increase Contrast | 减弱透明，实色 + separator | 不变 |

品牌色只用于：侧边栏选中、主按钮、开关开启（系统 accent）。

## 5. 视觉 token（实现集中在 `panel.rs`）

| Token | 值 |
| --- | --- |
| `UTILITY_WIDTH` / `UTILITY_HEIGHT` | 640 / 420 |
| `UTILITY_MIN_WIDTH` / `UTILITY_MIN_HEIGHT` | 560 / 360 |
| `SIDEBAR_WIDTH` | 172 |
| `DETAIL_INSET` | 28 |
| `DETAIL_MAX_WIDTH` | 400 |
| 侧边栏符号 | `moon.zzz` `display` `laptopcomputer` `checkmark.shield` `gearshape` `questionmark.circle` |
| 字体 | 系统 SF Pro：标题 17/22 bold，正文 13，脚注 11 |

## 6. Rust / AppKit 边界

```
Engine / ViewModel / Tr     策略、文案、JSON/CLI（Linux 可测）
panel.rs                    SidebarItem、token、PanelState
gui.rs                      窗口、状态项、IPC、快捷键
native_panel.rs             AppKit 视图树（仅 macOS）
```

状态仍走现有 `UiCommand` + `panel_state()`。不改 `status --json`、IPC `cmd`、CLI。

## 7. 与 HIG 冲突时的系统做法

- 全宽胶囊主按钮 → 固有宽度 Push。
- 设置页高密度卡片墙 → Sidebar + 稀疏详情（Notes）。
- 面板内退出 + 设置 + 返回 → 侧边栏导航；退出只在菜单。
- Accessory 无 App 菜单 → 命令放状态项菜单。
- 拟人太阳当详情主视觉 → 菜单栏保留 template 太阳；详情与侧边栏用 SF Symbols。
