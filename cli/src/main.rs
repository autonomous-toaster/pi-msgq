//! Command-line interface for the msgq message bus.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use msgq::commands::{self, AskArgs, ConsumersAction, CursorAction, FromSpec, PublishArgs};
use msgq::error::Error;

#[derive(Parser)]
#[command(
    name = "msgq",
    version,
    about = "Durable topic-addressed message bus for pi",
    long_about = None
)]
struct Cli {
    /// Database path (overrides MSGQ_DB and the default).
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Publish one message to an address.
    Publish {
        /// Target address: session/<id>, cwd/<enc>, group/<name>, cli/<who>,
        /// or a bare name (shorthand for group/<name>).
        address: String,
        /// Message body, or '-' to read it from stdin.
        message: Option<String>,
        /// Sender address (default: cli/<user>@<host>).
        #[arg(long)]
        from: Option<String>,
        /// Record type: prompt, steer, follow_up, or note.
        #[arg(long = "type", value_name = "TYPE")]
        kind: Option<String>,
        /// Treat MESSAGE (or stdin) as a full JSON record.
        #[arg(long)]
        json: bool,
        /// Request acknowledgements: `--ack` for every phase, or `--ack started,completed`.
        #[arg(long, value_name = "PHASES", num_args = 0..=1, default_missing_value = "true")]
        ack: Option<String>,
        /// The offset of the message this one answers.
        #[arg(long, value_name = "OFFSET")]
        reply_to: Option<i64>,
        /// Wait for acknowledgements after publishing.
        #[arg(long = "await")]
        await_ack: bool,
        /// Seconds to wait when awaiting; 0 waits without a limit.
        #[arg(long, value_name = "N", default_value_t = 300)]
        timeout: u64,
    },
    /// Ask another agent and wait for the answer.
    Ask {
        /// The address to ask.
        address: String,
        /// The question, or '-' to read it from stdin.
        message: Option<String>,
        /// Sender address (default: cli/<user>@<host>).
        #[arg(long)]
        from: Option<String>,
        /// Address the answer arrives on (default: this sender).
        #[arg(long, value_name = "ADDRESS")]
        on: Option<String>,
        /// Seconds to wait; 0 waits without a limit.
        #[arg(long, value_name = "N", default_value_t = 600)]
        timeout: u64,
        /// Emit the reply as its raw record, and outcomes as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Wait for acknowledgements of a published message.
    Await {
        /// The offset printed when the message was published.
        offset: i64,
        /// Address the acknowledgements arrive on (default: cli/<user>@<host>).
        #[arg(long, value_name = "ADDRESS")]
        on: Option<String>,
        /// Seconds to wait; 0 waits without a limit.
        #[arg(long, value_name = "N", default_value_t = 300)]
        timeout: u64,
        /// Emit one JSON object per acknowledgement.
        #[arg(long)]
        json: bool,
    },
    /// Read messages without touching any cursor.
    Read {
        #[arg(required = true, num_args = 1..)]
        addresses: Vec<String>,
        /// Start after this offset.
        #[arg(long, value_name = "N")]
        from: Option<i64>,
        /// Start from the earliest record.
        #[arg(long)]
        from_start: bool,
        /// Maximum number of records to print.
        #[arg(long, value_name = "N", default_value_t = 1000)]
        limit: i64,
        /// Emit raw records as JSON lines.
        #[arg(long)]
        json: bool,
    },
    /// Follow addresses, advancing a durable consumer cursor.
    Follow {
        #[arg(required = true, num_args = 1..)]
        addresses: Vec<String>,
        /// Consumer (subscription) name.
        #[arg(long)]
        consumer: String,
        /// Start after this offset.
        #[arg(long, value_name = "N")]
        from: Option<i64>,
        /// Start from the earliest record.
        #[arg(long)]
        from_start: bool,
        /// Emit raw records as JSON lines.
        #[arg(long)]
        json: bool,
    },
    /// List addresses that have messages.
    Topics {
        #[arg(long)]
        json: bool,
    },
    /// List live participants on the bus.
    Sessions {
        #[arg(long)]
        json: bool,
    },
    /// List or remove consumer cursors.
    Consumers {
        /// Restrict to one topic.
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        json: bool,
        #[command(subcommand)]
        action: Option<ConsumersCmd>,
    },
    /// Read, set, or reset one consumer cursor.
    Cursor {
        #[command(subcommand)]
        action: CursorCmd,
    },
    /// Database information.
    Db {
        #[command(subcommand)]
        action: DbCmd,
    },
}

#[derive(Subcommand)]
enum ConsumersCmd {
    /// Remove consumer cursors.
    Rm {
        name: String,
        /// Restrict to one topic.
        #[arg(long)]
        topic: Option<String>,
    },
}

#[derive(Subcommand)]
enum CursorCmd {
    /// Print a consumer's saved offset, or 0 when it has none.
    Get { consumer: String, topic: String },
    /// Save a consumer's offset (monotonic).
    Set {
        consumer: String,
        topic: String,
        offset: i64,
    },
    /// Clear a consumer's offset.
    Reset { consumer: String, topic: String },
}

#[derive(Subcommand)]
enum DbCmd {
    /// Show path, size, contents, and protocol version.
    Info,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("msgq: {e}");
            ExitCode::from(e.exit_code())
        }
    }
}

fn run(cli: Cli) -> Result<(), Error> {
    let db = msgq::config::resolve(cli.db.as_ref());
    match cli.command {
        Command::Publish {
            address,
            message,
            from,
            kind,
            json,
            ack,
            reply_to,
            await_ack,
            timeout,
        } => commands::publish(
            &db,
            PublishArgs {
                address,
                message,
                from,
                kind,
                json,
                ack,
                reply_to,
                await_ack,
                timeout,
            },
        ),
        Command::Ask {
            address,
            message,
            from,
            on,
            timeout,
            json,
        } => commands::ask(
            &db,
            AskArgs {
                address,
                message,
                from,
                on,
                timeout,
                json,
            },
        ),
        Command::Await {
            offset,
            on,
            timeout,
            json,
        } => commands::await_ack(&db, offset, on.as_deref(), timeout, json),
        Command::Read {
            addresses,
            from,
            from_start,
            limit,
            json,
        } => commands::read(&db, &addresses, from_spec(from, from_start)?, limit, json),
        Command::Follow {
            addresses,
            consumer,
            from,
            from_start,
            json,
        } => commands::follow(
            &db,
            &addresses,
            &consumer,
            from_spec(from, from_start)?,
            json,
        ),
        Command::Topics { json } => commands::topics(&db, json),
        Command::Sessions { json } => commands::sessions(&db, json),
        Command::Consumers {
            topic,
            json,
            action,
        } => {
            let action = match action {
                None => ConsumersAction::List,
                Some(ConsumersCmd::Rm { name, topic }) => ConsumersAction::Rm { name, topic },
            };
            commands::consumers(&db, topic.as_deref(), action, json)
        }
        Command::Cursor { action } => commands::cursor(
            &db,
            match action {
                CursorCmd::Get { consumer, topic } => CursorAction::Get { consumer, topic },
                CursorCmd::Set {
                    consumer,
                    topic,
                    offset,
                } => CursorAction::Set {
                    consumer,
                    topic,
                    offset,
                },
                CursorCmd::Reset { consumer, topic } => CursorAction::Reset { consumer, topic },
            },
        ),
        Command::Db { action } => match action {
            DbCmd::Info => commands::db_info(&db),
        },
    }
}

fn from_spec(from: Option<i64>, from_start: bool) -> Result<FromSpec, Error> {
    match (from, from_start) {
        (Some(_), true) => Err(Error::usage(
            "--from and --from-start are mutually exclusive",
        )),
        (Some(n), false) => Ok(FromSpec::Offset(n)),
        (None, true) => Ok(FromSpec::Start),
        (None, false) => Ok(FromSpec::End),
    }
}
