use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ovid_command_process::{ProcessScope, sample_scopes};
use tokio::time::{self, MissedTickBehavior};

use crate::command::COMMAND_RUNTIME;
use crate::workspace::sha256;

use super::{HOST, HostRecord, ScopeEntry, ScopeKind, write_record};

/// Records the processes of `scope` under `state_dir` while the scope lives.
///
/// One shared sampler on the command runtime reads every tracked scope each 500 ms. It rewrites
/// a record file only when the processes of one of its scopes change. Without a state directory,
/// nothing is recorded.
pub(crate) fn track(
    state_dir: Option<&Path>,
    scope: &Arc<ProcessScope>,
    kind: ScopeKind,
    owner_tag: Option<&str>,
    command: &str,
) {
    let (Some(state_dir), Some(_), Ok(runtime)) = (state_dir, HOST.as_ref(), &*COMMAND_RUNTIME)
    else {
        return;
    };
    let mut tracker = tracker();
    let id = tracker.next_id;
    tracker.next_id += 1;
    tracker.scopes.insert(
        id,
        Tracked {
            directory: state_dir.join("processes"),
            scope: Arc::downgrade(scope),
            entry: ScopeEntry {
                kind,
                owner_tag: owner_tag.map(str::to_owned),
                command_hash: sha256(command.as_bytes()),
                started_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |elapsed| {
                        elapsed.as_millis().try_into().unwrap_or(u64::MAX)
                    }),
                group: None,
                members: Vec::new(),
            },
        },
    );
    if !tracker.running {
        tracker.running = true;
        runtime.spawn(sample_until_idle());
    }
}

/// Stops every process of `scope`, as [`ProcessScope::terminate`] does, and removes its record.
pub(crate) fn terminate(scope: &ProcessScope, grace: Duration) -> io::Result<()> {
    let stopped = scope.terminate(grace);
    let mut tracker = tracker();
    let Some(id) = tracker
        .scopes
        .iter()
        .find_map(|(id, tracked)| std::ptr::eq(tracked.scope.as_ptr(), scope).then_some(*id))
    else {
        return stopped;
    };
    if let Some(removed) = tracker.scopes.remove(&id)
        && !removed.entry.members.is_empty()
    {
        persist(&tracker, &removed.directory);
    }
    stopped
}

struct Tracker {
    next_id: u64,
    running: bool,
    scopes: BTreeMap<u64, Tracked>,
}

struct Tracked {
    directory: PathBuf,
    scope: Weak<ProcessScope>,
    entry: ScopeEntry,
}

static TRACKER: Mutex<Tracker> = Mutex::new(Tracker {
    next_id: 0,
    running: false,
    scopes: BTreeMap::new(),
});

fn tracker() -> MutexGuard<'static, Tracker> {
    TRACKER.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn sample_until_idle() {
    let mut interval = time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        match tokio::task::spawn_blocking(sample).await {
            Ok(true) => (),
            Ok(false) => break,
            Err(_) => {
                tracker().running = false;
                break;
            }
        }
    }
}

/// Samples every tracked scope, rewrites the changed record files, and returns whether any
/// scope is still tracked.
fn sample() -> bool {
    let mut changed = BTreeSet::new();
    let mut scopes = Vec::new();
    tracker()
        .scopes
        .retain(|id, tracked| match tracked.scope.upgrade() {
            Some(scope) => {
                scopes.push((*id, scope));
                true
            }
            None => {
                if !tracked.entry.members.is_empty() {
                    changed.insert(tracked.directory.clone());
                }
                false
            }
        });
    // Sampling waits for each scope lock, and a terminating scope holds its lock for its grace.
    let samples = sample_scopes(&scopes.iter().map(|(_, scope)| &**scope).collect::<Vec<_>>());

    let mut tracker = tracker();
    for ((id, _), sample) in scopes.iter().zip(samples) {
        let Some(tracked) = tracker.scopes.get_mut(id) else {
            continue;
        };
        if (tracked.entry.group, &tracked.entry.members) != (sample.group, &sample.members) {
            changed.insert(tracked.directory.clone());
            tracked.entry.group = sample.group;
            tracked.entry.members = sample.members;
        }
    }
    for directory in &changed {
        persist(&tracker, directory);
    }
    tracker.running = !tracker.scopes.is_empty();
    tracker.running
}

fn persist(tracker: &Tracker, directory: &Path) {
    let Some(host) = HOST.as_ref() else {
        return;
    };
    let record = HostRecord {
        entries: tracker
            .scopes
            .values()
            .filter(|tracked| tracked.directory == directory && !tracked.entry.members.is_empty())
            .map(|tracked| tracked.entry.clone())
            .collect(),
        ..host.clone()
    };
    // A command must not fail because its safety net cannot write. The next change writes again.
    let _ = write_record(directory, &record);
}
