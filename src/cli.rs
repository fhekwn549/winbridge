use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "winbridge", version, about = "Linux-native KakaoTalk manager")]
pub struct Cli {
    #[arg(long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start or resume the VM and open the RDP window.
    Start,

    /// Close the RDP window and pause the VM.
    Stop {
        /// Shut down the guest OS instead of managed-save.
        #[arg(long)]
        shutdown: bool,
    },

    /// Print the VM state.
    Status,
}
