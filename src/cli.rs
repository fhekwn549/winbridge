use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "winbridge", version, about = "Linux-native Windows app bridge")]
pub struct Cli {
    #[arg(long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start or resume the VM and open the RDP window.
    Start {
        /// Window mode to open after the VM is ready.
        #[arg(long, value_enum, default_value_t = WindowMode::App)]
        mode: WindowMode,

        /// RDP display strategy to use for app mode experiments.
        #[arg(long, value_enum, default_value_t = DisplayStrategy::StableSlots)]
        display: DisplayStrategy,
    },

    /// Close the RDP window and pause the VM.
    Stop {
        /// Shut down the guest OS instead of managed-save.
        #[arg(long)]
        shutdown: bool,
    },

    /// Install the winbridge desktop launcher and icon for the current user.
    InstallDesktopEntry {
        /// winbridge executable path to put in the desktop launcher.
        #[arg(long)]
        exec: Option<PathBuf>,
    },

    /// Remove the winbridge desktop launcher and icon for the current user.
    UninstallDesktopEntry,

    /// Print the VM state.
    Status,

    /// Diagnose host, VM, and RDP readiness.
    Doctor,

    /// Write a diagnostic bundle for troubleshooting.
    DiagnosticBundle {
        /// Output file path. Defaults to ~/.cache/winbridge/diagnostics/.
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Restart and foreground the Winbridge app through QEMU guest agent.
    RepairKakao,

    /// Restore Windows wallpaper from a reachable source or theme cache.
    RepairWallpaper,

    /// Install Windows http/https forwarding so guest links open on the Linux host.
    InstallUrlForwarder,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMode {
    App,
    Desktop,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayStrategy {
    StableSlots,
    ExperimentalMultimon,
}
