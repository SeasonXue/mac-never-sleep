use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "never-sleep",
    version,
    about = "Never Sleep: turn the Mac display off and keep the machine awake for ChatGPT / Codex remote sessions"
)]
pub struct Cli {
    /// Start the menu bar (default: no subcommand in a macOS GUI session)
    #[arg(long)]
    pub menubar: bool,

    /// UI language: en (default) or zh. Overrides the saved preference for this process.
    #[arg(long, global = true, value_name = "LANG")]
    pub lang: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start standby (talk to the menu bar if it is running, otherwise occupy this process)
    On {
        /// Duration: indefinite, 3h, until=08:00
        #[arg(long)]
        r#for: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// End standby
    Off {
        #[arg(long)]
        json: bool,
    },
    /// Toggle standby
    Toggle {
        #[arg(long)]
        json: bool,
    },
    /// Show status
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Diagnose power / lid / assertions
    Doctor,
    /// Restore clamshell-sleep flags (safety net after a crash)
    Cleanup,
    /// Print usage notes
    Explain,
}
