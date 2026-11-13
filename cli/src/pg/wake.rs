//! Waking a follower.
//!
//! The engine notifies on insert, throttled per queue. A listening connection is an optimisation,
//! not a requirement: a pooler in transaction mode cannot keep a LISTEN alive, so the fallback is
//! to wait out the slice and let the caller look again.

use std::sync::Mutex;
use std::time::Duration;

use fallible_iterator::FallibleIterator;
use postgres::Client;

use super::naming::{escaped, queue_name};
use super::schema::pg;
use crate::error::Error;
use crate::store::Wake;

/// A wake source fed by the engine's insert notification, with polling underneath.
pub struct PgWake {
    listener: Option<Mutex<Client>>,
}

/// A wake source for the named topics, listening when the connection can.
pub fn source(config: postgres::Config, topics: &[&str]) -> Box<dyn Wake> {
    Box::new(PgWake {
        listener: listen(config, topics).map(Mutex::new),
    })
}

impl Wake for PgWake {
    fn wait(&self, slice: Duration) -> Result<(), Error> {
        let Some(client) = &self.listener else {
            // No listening connection: waiting out the slice is the same contract, just slower.
            std::thread::sleep(slice);
            return Ok(());
        };
        let mut client = client
            .lock()
            .map_err(|_| Error::runtime("the wake connection was poisoned"))?;
        let mut notifications = client.notifications();
        let mut waiting = notifications.timeout_iter(slice);
        // A notification and a timeout are both a reason to look again, so the result only matters
        // when the server has gone away.
        waiting.next().map_err(pg)?;
        Ok(())
    }
}

/// A connection registered for the insert notifications of the given topics.
fn listen(config: postgres::Config, topics: &[&str]) -> Option<Client> {
    let mut client = config.connect(postgres::NoTls).ok()?;
    for topic in topics {
        let name = queue_name(topic);
        let channel = format!("pgmq.q_{}.INSERT", escaped(&name));
        client
            .batch_execute(&format!("LISTEN \"{channel}\""))
            .ok()?;
    }
    Some(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wake_without_a_listener_waits_out_its_slice() {
        // The fallback contract: no notification, so the caller is handed back after one slice and
        // re-reads. That is what polling is, and it must not return early or fail.
        let poll = PgWake { listener: None };
        let started = std::time::Instant::now();
        poll.wait(Duration::from_millis(40)).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(35));
    }
}
