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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_on_for_and_json() {
        let cli = Cli::try_parse_from(["never-sleep", "on", "--for", "8h", "--json"]).unwrap();
        match cli.command {
            Some(Command::On { r#for, json }) => {
                assert_eq!(r#for.as_deref(), Some("8h"));
                assert!(json);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_global_lang_before_status() {
        let cli = Cli::try_parse_from(["never-sleep", "--lang", "zh", "status"]).unwrap();
        assert_eq!(cli.lang.as_deref(), Some("zh"));
        assert!(matches!(cli.command, Some(Command::Status { json: false })));
    }

    #[test]
    fn menubar_flag_has_no_subcommand() {
        let cli = Cli::try_parse_from(["never-sleep", "--menubar"]).unwrap();
        assert!(cli.menubar);
        assert!(cli.command.is_none());
    }

    #[test]
    fn doctor_cleanup_explain_have_no_json_flag() {
        for args in [
            ["never-sleep", "doctor"].as_slice(),
            ["never-sleep", "cleanup"].as_slice(),
            ["never-sleep", "explain"].as_slice(),
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(cli.command.is_some());
        }
    }
}
