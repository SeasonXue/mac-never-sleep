# Never Sleep — macOS UI

对照 `docs/screenshots`：紧凑菜单栏面板。系统骨架 + 拟人太阳/月亮。不是侧边栏工具窗。

## 窗口

Accessory + 菜单栏 Extra。左键显示/隐藏面板；右键状态项菜单。不占 Dock。不吸附图标。失焦不关。Escape / 关面板不结束待命。

尺寸 **320×480**，圆角 10，无标题栏。首次打开居中。

## 三页（截图）

**主界面**
- 居中 124pt 圆形硬币：未开启太阳 / 待命中月亮（`ui/assets`）
- 17pt 居中标题 + 12pt 灰色摘要
- 全宽 28pt Push：未开启为默认强调色「开始关屏待命」；待命中为普通「结束待命」（不置灰）
- 白底圆角分组卡：时长弹出菜单、人离开后再关屏、电量低于 20%
- 底栏：更多设置 | 退出

**设置**（返回 chevron + 居中标题）
- 同一张分组卡：立即关屏、合盖、再关屏、锁登录、电量、登录启动
- 语言分段：English / 简体中文
- 底栏：使用说明 | 退出

**使用说明**
- 蓝色 kicker + 加粗导语
- 「怎么用」编号 1–3 卡片（第三步 `⌥⌘P` 键帽）
- 「请留意」SF Symbols + 短文

## 材质

玻璃只包外壳（`NSGlassEffectView` / `NSVisualEffectMaterial::Popover`）。卡片、正文、开关用 `controlBackgroundColor` / 系统语义色。

## Token（`panel.rs`）

| Token | 值 |
| --- | --- |
| `PANEL_WIDTH` / `PANEL_HEIGHT` | 320 / 480 |
| `HERO_SIZE` / `HERO_IMAGE` | 124 / 104 |
| `CARD_RADIUS` | 8 |
| `CONTENT_INSET` | 16 |
