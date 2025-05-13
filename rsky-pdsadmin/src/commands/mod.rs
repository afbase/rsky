pub mod account;
pub mod create_invite_code;
pub mod request_crawl;
pub mod rsky_pds;
pub mod update;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

/// RSKY PDS Administration CLI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Enable verbose logging for additional debugging information
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Account management commands
    Account {
        #[command(subcommand)]
        subcommand: account::AccountCommands,
    },

    /// Create an invite code
    #[command(name = "create-invite-code")]
    CreateInviteCode,

    /// Request a crawl from a relay
    #[command(name = "request-crawl")]
    RequestCrawl {
        /// Comma-separated list of relay hosts
        #[arg(default_value = "")]
        relay_hosts: String,
    },

    /// Update the PDS to the latest version
    Update {
        /// Target version to update to (optional)
        #[arg(default_value = "")]
        target_version: String,
    },

    /// RSKY-PDS specific commands
    #[command(name = "rsky-pds")]
    RskyPds {
        #[command(subcommand)]
        subcommand: rsky_pds::RskyPdsCommands,
    },

    /// Display help information
    Help,
}

/// Set whether verbose logging is enabled
pub static mut VERBOSE_LOGGING: bool = false;

/// Check if verbose logging is enabled
pub fn is_verbose() -> bool {
    unsafe { VERBOSE_LOGGING }
}

/// Execute the CLI command
pub fn execute() -> Result<()> {
    let cli = Cli::parse();

    // Set the verbose flag for global use
    unsafe {
        VERBOSE_LOGGING = cli.verbose;
    }

    // If verbose mode is enabled, display it
    if cli.verbose {
        println!("Verbose mode enabled");
    }

    match &cli.command {
        Commands::Account { subcommand } => {
            account::execute(subcommand).context("Failed to execute account command")
        }
        Commands::CreateInviteCode => {
            create_invite_code::execute().context("Failed to create invite code")
        }
        Commands::RequestCrawl { relay_hosts } => {
            request_crawl::execute(relay_hosts).context("Failed to request crawl")
        }
        Commands::Update { target_version } => {
            update::execute(target_version).context("Failed to update PDS")
        }
        Commands::RskyPds { subcommand } => {
            rsky_pds::execute(subcommand).context("Failed to execute rsky-pds command")
        }
        Commands::Help => {
            print_help();
            Ok(())
        }
    }
}

/// Print the help information
fn print_help() {
    println!("pdsadmin help");
    println!("--");
    println!("update");
    println!("  Update to the latest PDS version.");
    println!("    e.g. pdsadmin update");
    println!();
    println!("account");
    println!("  list");
    println!("    List accounts");
    println!("    e.g. pdsadmin account list");
    println!("  create <EMAIL> <HANDLE>");
    println!("    Create a new account");
    println!("    e.g. pdsadmin account create alice@example.com alice.example.com");
    println!("  delete <DID>");
    println!("    Delete an account specified by DID.");
    println!("    e.g. pdsadmin account delete did:plc:xyz123abc456");
    println!("  takedown <DID>");
    println!("    Takedown an account specified by DID.");
    println!("    e.g. pdsadmin account takedown did:plc:xyz123abc456");
    println!("  untakedown <DID>");
    println!("    Remove a takedown from an account specified by DID.");
    println!("    e.g. pdsadmin account untakedown did:plc:xyz123abc456");
    println!("  reset-password <DID>");
    println!("    Reset a password for an account specified by DID.");
    println!("    e.g. pdsadmin account reset-password did:plc:xyz123abc456");
    println!();
    println!("request-crawl [<RELAY HOST>]");
    println!("    Request a crawl from a relay host.");
    println!("    e.g. pdsadmin request-crawl bsky.network");
    println!();
    println!("create-invite-code");
    println!("  Create a new invite code.");
    println!("    e.g. pdsadmin create-invite-code");
    println!();
    println!("rsky-pds");
    println!("  init-db");
    println!("    Initialize the database with the required schema.");
    println!("    e.g. pdsadmin rsky-pds init-db");
    println!();
    println!("help");
    println!("    Display this help information.");
}
