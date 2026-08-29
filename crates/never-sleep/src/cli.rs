use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "never-sleep",
    version,
    about = "熄屏待命：关掉 Mac 屏幕、不让电脑睡眠，方便 ChatGPT / Codex 远程连接"
)]
pub struct Cli {
    /// 启动菜单栏（默认：无子命令且在 macOS 图形会话时）
    #[arg(long)]
    pub menubar: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 开始待命（若菜单栏已运行则发指令，否则前台占用本进程）
    On {
        /// 时长：indefinite、3h、until=08:00
        #[arg(long)]
        r#for: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// 结束待命
    Off {
        #[arg(long)]
        json: bool,
    },
    /// 切换待命
    Toggle {
        #[arg(long)]
        json: bool,
    },
    /// 查看状态
    Status {
        #[arg(long)]
        json: bool,
    },
    /// 诊断电源 / 合盖 / 断言
    Doctor,
    /// 还原合盖睡眠标志（进程崩溃后的保险）
    Cleanup,
    /// 打印使用说明
    Explain,
}
