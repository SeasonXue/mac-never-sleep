# Never Sleep — macOS UI

对照 `docs/screenshots`：紧凑菜单栏弹出面板。系统骨架 + 拟人太阳/月亮。不是侧边栏工具窗。

## 窗口

Accessory + 菜单栏 Extra。左键把 **320×391** 圆角卡片吸附在状态项下方；失焦、Escape、再点图标会隐藏面板，**不结束待命**。右键状态项菜单。不占 Dock。窗口比卡片大一圈（`SHADOW_INSET` 40pt），给四周柔和层阴影留出发光空间。阴影画在卡片层上；待命时 Dark Aqua 只加在圆角内容上，窗口透明区保持透明，阴影不会被涂没。

待命中面板铺上 `#1c1c1e`，未开启为 `#f5f5f7`，420ms 过渡。硬币是真的半圈翻转：月亮贴在背面（`rotation.y = π`），容器绕 Y 轴 0↔π；`CATransformLayer` + 中心锚点，透视 `m34`。第一次绘制先 `setHidden` 月亮，避免两面重合。系统「减少动态效果」时立刻换面。不要把手工 `CATransform3D` 塞进 `msg_send`，也不要用 `scale.x` 压扁。

## 三页（截图）

**主界面**
- 居中 124pt 圆形硬币：未开启太阳 / 待命中月亮（`ui/assets`）
- 17pt 居中标题 + 12pt 灰色摘要
- 全宽 28pt Push：未开启为默认强调色「开始关屏待命」；待命中为「结束待命」（不置灰）
- 待命中标题下：无限期显示已过时长（`0:00`）；1/3/8 小时或到 08:00 则显示剩余倒计时（`1:00:00`），并多一颗「立即熄屏」马上关屏、不结束待命
- 底栏：更多设置 | 退出

**设置**（返回 chevron + 居中标题）
- 同一张分组卡：时长、立即关屏、合盖、再关屏、锁登录、电量、登录启动
- 语言分段：English / 简体中文
- 底栏：使用说明 | 退出

**使用说明**
- 蓝色 kicker + 加粗导语
- 「怎么用」编号 1–3 卡片（第三步 `⌥⌘P` 键帽）
- 「请留意」SF Symbols + 短文

## 材质

玻璃只包外壳（`NSGlassEffectView` / `NSVisualEffectMaterial::Popover`）。卡片、正文、开关用 `controlBackgroundColor` / 系统语义色。待命时切到 Dark Aqua，让分组卡和开关跟着重色面板走。

## Token（`panel.rs`）

| Token | 值 |
| --- | --- |
| `PANEL_WIDTH` / `PANEL_HEIGHT` | 320 / 391 |
| `SHADOW_INSET` / `PANEL_CORNER` | 40 / 10 |
| `HERO_SIZE` / `HERO_IMAGE` | 124 / 104 |
| `CARD_RADIUS` | 8 |
| `CONTENT_INSET` | 16 |
| `PRIMARY_HEIGHT` / `PRIMARY_CLUSTER_GAP` | 28 / 8 |
| `HERO_FLIP_SECS` / `PANEL_COLOR_SECS` | 0.52 / 0.42 |
