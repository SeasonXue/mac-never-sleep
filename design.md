# Never Sleep — macOS UI

对照 `docs/screenshots`：紧凑菜单栏弹出面板。系统骨架 + 拟人太阳/月亮。不是侧边栏工具窗。

## 窗口

Accessory + 菜单栏 Extra。左键把 **320×480** 圆角卡片吸附在状态项下方；失焦、Escape、再点图标会隐藏面板，**不结束待命**。右键状态项菜单。不占 Dock。窗口比卡片大一圈（`SHADOW_INSET` 24pt），给四周柔和层阴影留出发光空间。

待命中面板铺上 `#1c1c1e`，未开启为 `#f5f5f7`，420ms 过渡。硬币待命时换成月亮（淡入淡出；系统「减少动态效果」时立刻换图）。不要叠两张脸做 3D `rotateY`：objc2 里传 `CATransform3D` 会同时露出太阳和月亮，点击还会闪退。

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

玻璃只包外壳（`NSGlassEffectView` / `NSVisualEffectMaterial::Popover`）。卡片、正文、开关用 `controlBackgroundColor` / 系统语义色。待命时切到 Dark Aqua，让分组卡和开关跟着重色面板走。

## Token（`panel.rs`）

| Token | 值 |
| --- | --- |
| `PANEL_WIDTH` / `PANEL_HEIGHT` | 320 / 480 |
| `SHADOW_INSET` / `PANEL_CORNER` | 24 / 10 |
| `HERO_SIZE` / `HERO_IMAGE` | 124 / 104 |
| `CARD_RADIUS` | 8 |
| `CONTENT_INSET` | 16 |
| `HERO_FLIP_SECS` / `PANEL_COLOR_SECS` | 0.52 / 0.42 |
