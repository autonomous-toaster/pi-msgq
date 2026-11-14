//! The command definition: what the CLI accepts, and nothing about what it does.
//!
//! It lives beside the emitter rather than in the binary because the emitter walks this tree and the
//! tests bind the two together: a flag, a subcommand or a closed set cannot exist on one side only.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::card::PathMode;
use crate::record;

#[derive(Parser)]
#[command(
    name = "msgq",
    version,
    about = "Durable topic-addressed message bus for pi",
    long_about = None
)]
pub struct Cli {
    /// Database path (overrides MSGQ_DB and the default).
    #[arg(long, global = true, value_name = "PATH")]
    pub db: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

/// The shells this CLI describes. One, deliberately: a script nobody tests is not support.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Shell {
    Zsh,
}

/// The phases a receiver may be asked to report. See `PROTOCOL.md`.
const ACK_PHASES: [&str; 3] = ["received", "started", "completed"];

/// Parse `--ack`: `true` for every phase, or a comma-separated subset of the protocol phases.
///
/// Validated here rather than after a publish, because a phase list that no receiver can report would
/// otherwise publish a record that asks for nothing.
fn parse_ack(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw == "true" {
        return Ok(raw.to_string());
    }
    if raw.is_empty() {
        return Err("an acknowledgement phase list must name at least one phase".to_string());
    }
    for phase in raw.split(',') {
        let phase = phase.trim();
        if !ACK_PHASES.contains(&phase) {
            return Err(format!(
                "'{phase}' is not an acknowledgement phase; expected received, started, or completed"
            ));
        }
    }
    Ok(raw.to_string())
}

#[derive(Subcommand)]
pub enum Command {
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
        /// Record type.
        #[arg(long = "type", value_name = "TYPE")]
        kind: Option<record::Kind>,
        /// Treat MESSAGE (or stdin) as a full JSON record.
        #[arg(long)]
        json: bool,
        /// Request acknowledgements: `--ack` for every phase, or `--ack started,completed`.
        #[arg(long, value_name = "PHASES", num_args = 0..=1, default_missing_value = "true",
              value_parser = parse_ack)]
        ack: Option<String>,
        /// The offset of the message this one answers, or its id `<address>#<offset>`.
        #[arg(long, value_name = "ID")]
        reply_to: Option<String>,
        /// Print the record's id instead of its offset.
        #[arg(long = "print-id")]
        print_id: bool,
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
        /// Publish the question and return without waiting for the answer.
        #[arg(long = "no-wait")]
        no_wait: bool,
    },
    /// Wait for acknowledgements of a published message.
    Await {
        /// A record id (address#offset), or a bare offset together with --on.
        target: String,
        /// Address the acknowledgements arrive on; required with a bare offset.
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
        /// Wrap each JSON line as {topic, offset, created_at, record}.
        #[arg(long, requires = "json")]
        with_offset: bool,
        /// Exit non-zero when retention removed records the caller asked for.
        #[arg(long)]
        strict: bool,
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
        /// Wrap each JSON line as {topic, offset, created_at, record}.
        #[arg(long, requires = "json")]
        with_offset: bool,
    },
    /// List addresses that have messages.
    Topics {
        #[arg(long)]
        json: bool,
        /// Print one address per line, with no count and no offset.
        #[arg(long, conflicts_with = "json")]
        names: bool,
    },
    /// List live participants on the bus.
    Sessions {
        #[arg(long)]
        json: bool,
        /// Show the locator column: project, branch, and path.
        #[arg(long)]
        wide: bool,
        /// Print one participant address per line.
        #[arg(long, conflicts_with = "json")]
        names: bool,
    },
    /// Print the peer locator derived from a working directory.
    Card {
        /// Directory to describe (default: the current directory).
        #[arg(long, value_name = "PATH")]
        cwd: Option<PathBuf>,
        /// Path detail.
        #[arg(long, value_name = "MODE")]
        paths: Option<PathMode>,
        /// Also report the addresses a participant publishing this card would answer at.
        #[arg(long, value_name = "ADDR")]
        address: Option<String>,
    },
    /// List or remove consumer cursors.
    Consumers {
        /// Restrict to one topic.
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        json: bool,
        /// Print one consumer name per line.
        #[arg(long, conflicts_with = "json")]
        names: bool,
        #[command(subcommand)]
        action: Option<ConsumersCmd>,
    },
    /// Read, set, or reset one consumer cursor.
    Cursor {
        #[command(subcommand)]
        action: CursorCmd,
    },
    /// Print a shell completion script on standard output.
    Completions {
        /// The shell to describe.
        #[arg(value_name = "SHELL")]
        shell: Shell,
    },
    /// Database information.
    Db {
        #[command(subcommand)]
        action: DbCmd,
    },
    /// Remove records older than an age, never one a consumer has not passed.
    Prune {
        /// Age: a number and one of d, h, m, s. Records older than it are removable.
        #[arg(long, value_name = "AGE")]
        older_than: Option<String>,
        /// Retention window to apply instead of the configured one.
        #[arg(long, value_name = "AGE")]
        retention: Option<String>,
        /// Remove what would be removed, instead of reporting it.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand)]
pub enum ConsumersCmd {
    /// Remove consumer cursors.
    Rm {
        name: String,
        /// Restrict to one topic.
        #[arg(long)]
        topic: Option<String>,
    },
    /// Collect the cursors of sessions that are no longer live.
    Gc {
        /// Remove them, instead of reporting how many there are.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand)]
pub enum CursorCmd {
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
pub enum DbCmd {
    /// Show path, size, contents, and protocol version.
    Info,
}
#[cfg(test)]
mod completion_tests {
    use super::*;
    use crate::completion;
    use clap::CommandFactory;

    fn tree() -> clap::Command {
        Cli::command()
    }

    fn script() -> String {
        completion::script(&tree())
    }

    #[test]
    fn every_provider_names_a_real_command_and_argument() {
        for (path, id, _) in completion::PROVIDERS {
            let mut node = tree();
            for segment in path.split(' ').filter(|s| !s.is_empty()) {
                node = node
                    .find_subcommand(segment)
                    .unwrap_or_else(|| panic!("no subcommand {segment:?} on {path:?}"))
                    .clone();
            }
            let found = node
                .get_arguments()
                .any(|arg| arg.get_long().unwrap_or(arg.get_id().as_str()) == *id);
            assert!(found, "{path:?} has no argument {id:?}");
        }
    }

    #[test]
    fn the_script_offers_what_the_definition_declares() {
        let script = script();
        assert_eq!(script, self::script(), "emission is deterministic");

        let mut stack = vec![(tree(), String::new())];
        while let Some((node, path)) = stack.pop() {
            let name = node.get_name().to_string();
            let function = if path.is_empty() {
                String::new()
            } else {
                format!("{path}_{name}")
            };
            if !function.is_empty() && name != "help" {
                // Every command reachable from the prompt has a function of its own, named by its path.
                assert!(
                    script.contains(&format!("_msgq_{function}()")),
                    "no dispatch for {function}"
                );
            }
            for arg in node.get_arguments() {
                if let Some(long) = arg.get_long() {
                    if long == "help" || long == "version" {
                        continue;
                    }
                    assert!(
                        script.contains(&format!("--{long}[")),
                        "{name} --{long} is not emitted"
                    );
                }
                if !arg.get_action().takes_values() {
                    // A boolean flag takes no value, so it carries no closed set to complete.
                    continue;
                }
                let names: Vec<String> = arg
                    .get_possible_values()
                    .iter()
                    .map(|value| value.get_name().to_string())
                    .collect();
                if !names.is_empty() {
                    // A closed set completes to its members, in the order the definition declares.
                    assert!(
                        script.contains(&format!("({})", names.join(" "))),
                        "{name}: the closed set {names:?} is not emitted"
                    );
                }
            }
            // The root's children are named by their own name, not by "msgq_publish".
            let next = function.clone();
            stack.extend(
                node.get_subcommands()
                    .filter(|sub| sub.get_name() != "help")
                    .cloned()
                    .map(|sub| (sub, next.clone())),
            );
        }
    }

    #[test]
    fn a_pattern_value_is_offered_without_being_closed() {
        let script = script();
        assert!(
            script.contains("_msgq_hint_publish_ack"),
            "the phase list is offered"
        );
        assert!(
            !script.contains(":value:(true received started"),
            "a pattern value must not become a closed set"
        );
    }

    #[test]
    fn the_dynamic_arguments_are_the_declared_ones() {
        let script = script();
        for helper in [
            "_msgq_offer_addresses",
            "_msgq_offer_topics",
            "_msgq_offer_consumers",
        ] {
            assert!(
                script.contains(&format!("{helper}() {{")),
                "{helper} is missing"
            );
        }
        assert!(script.contains(":_msgq_offer_addresses"));
        assert!(script.contains(":_msgq_offer_consumers"));
        assert!(script.contains(":_msgq_offer_topics"));
    }
}
