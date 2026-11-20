//! Command-line interface for the msgq message bus.

use std::process::ExitCode;

use clap::Parser;
use serde_json::json;

use msgq::address;
use msgq::card::{self, PathMode};
use msgq::cli::{Cli, Command, ConsumersCmd, CursorCmd, DbCmd, Shell};
use msgq::commands::{self, AskArgs, ConsumersAction, CursorAction, FromSpec, PublishArgs};
use msgq::completion;
use msgq::error::Error;

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
    // The command tree is the argument to the emitter, so `msgq completions` describes the CLI the
    // parser runs rather than a copy of it.
    let definition = || <Cli as clap::CommandFactory>::command();
    let db = msgq::config::resolve_target(cli.db.as_ref())?;
    match cli.command {
        Command::Publish {
            address,
            message,
            from,
            kind,
            json,
            ack,
            reply_to,
            print_id,
            await_ack,
            timeout,
        } => commands::publish(
            &db,
            PublishArgs {
                address,
                message,
                from,
                kind: kind.map(|k| k.as_str().to_string()),
                json,
                ack,
                reply_to,
                print_id,
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
            no_wait,
        } => commands::ask(
            &db,
            AskArgs {
                address,
                message,
                from,
                on,
                timeout,
                json,
                no_wait,
            },
        ),
        Command::Await {
            target,
            on,
            timeout,
            json,
        } => commands::await_ack(&db, &target, on.as_deref(), timeout, json),
        Command::Read {
            addresses,
            from,
            from_start,
            limit,
            json,
            with_offset,
            strict,
        } => commands::read(
            &db,
            &addresses,
            from_spec(from, from_start)?,
            limit,
            json,
            with_offset,
            strict,
        ),
        Command::Follow {
            addresses,
            consumer,
            from,
            from_start,
            json,
            with_offset,
        } => commands::follow(
            &db,
            &addresses,
            &consumer,
            from_spec(from, from_start)?,
            json,
            with_offset,
        ),
        Command::Topics { json, names, card } => commands::topics(&db, json, names, card),
        Command::Sessions {
            json,
            wide,
            names,
            card,
        } => commands::sessions(&db, json, wide, names, card),
        Command::Card {
            cwd,
            paths,
            address,
        } => {
            let address = match address {
                Some(raw) => Some(address::canonicalize(&raw)?),
                None => None,
            };
            let cwd = match cwd {
                Some(path) => path,
                None => std::env::current_dir()?,
            };
            let card = card::derive_card(&cwd, paths.unwrap_or(PathMode::Relative))?;
            match address {
                // An address is a separate fact from the locator, so it is reported beside it
                // rather than merged into it: the published `event.card` keeps its four fields.
                Some(address) => println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "address": address,
                        "card": card,
                        "locator": card::summary(&card),
                    }))?
                ),
                None => println!("{}", serde_json::to_string(&card)?),
            }
            Ok(())
        }
        Command::Prune {
            older_than,
            retention,
            apply,
        } => {
            let window = older_than.or(retention);
            commands::prune(&db, commands::PruneArgs { window, apply })
        }
        Command::Consumers {
            topic,
            json,
            names,
            action,
        } => {
            let action = match action {
                None => ConsumersAction::List,
                Some(ConsumersCmd::Rm { name, topic }) => ConsumersAction::Rm { name, topic },
                Some(ConsumersCmd::Gc { apply }) => {
                    return commands::consumers_gc(&db, apply);
                }
            };
            commands::consumers(&db, topic.as_deref(), action, json, names)
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
        Command::Completions { shell } => {
            match shell {
                Shell::Zsh => print!("{}", completion::script(&definition())),
            }
            Ok(())
        }
        Command::Keygen { principal, force } => commands::keygen(principal, force),
        Command::Fingerprint { principal } => commands::fingerprint(principal),
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
