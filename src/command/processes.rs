#[cfg(windows)]
mod platform {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use ovid_command_process::{ProcessIdentity, current_pid, descendants, is_running, terminate};
    use tokio::time;

    pub(crate) struct SpawnRegistry {
        baseline: HashSet<ProcessIdentity>,
        identities: Mutex<HashSet<ProcessIdentity>>,
    }

    impl SpawnRegistry {
        pub(crate) fn new() -> Self {
            Self {
                baseline: descendants(current_pid()).into_iter().collect(),
                identities: Mutex::new(HashSet::new()),
            }
        }

        pub(crate) async fn terminate(self: &Arc<Self>) {
            for pause in [10, 25, 75] {
                if !self.terminate_wave() {
                    return;
                }
                time::sleep(Duration::from_millis(pause)).await;
            }
        }

        fn terminate_wave(&self) -> bool {
            let Ok(mut identities) = self.identities.lock() else {
                return false;
            };
            let mut seen = HashSet::new();
            let mut targets = Vec::new();
            for identity in descendants(current_pid()) {
                if !self.baseline.contains(&identity) && seen.insert(identity) {
                    targets.push(identity);
                }
            }
            for identity in identities.iter().copied() {
                if is_running(identity) && seen.insert(identity) {
                    targets.push(identity);
                }
            }
            identities.extend(targets.iter().copied());
            drop(identities);

            for identity in &targets {
                let _ = terminate(*identity);
            }
            !targets.is_empty()
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use sysinfo::{Pid, ProcessesToUpdate, System};
    use tokio::time;

    pub(crate) struct SpawnRegistry {
        baseline: HashMap<Pid, u64>,
        identities: Mutex<HashMap<Pid, u64>>,
    }

    impl SpawnRegistry {
        pub(crate) fn new() -> Self {
            let system = process_system();
            let baseline = sysinfo::get_current_pid()
                .ok()
                .map(|pid| descendant_order(&system, &[pid]))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|pid| {
                    system
                        .process(pid)
                        .map(|process| (pid, process.start_time()))
                })
                .collect();
            Self {
                baseline,
                identities: Mutex::new(HashMap::new()),
            }
        }

        pub(crate) async fn terminate(self: &Arc<Self>) {
            for pause in [25, 75, 150] {
                let registry = Arc::clone(self);
                let Ok(active) =
                    tokio::task::spawn_blocking(move || registry.terminate_wave()).await
                else {
                    return;
                };
                if !active {
                    return;
                }
                time::sleep(Duration::from_millis(pause)).await;
            }
        }

        fn terminate_wave(&self) -> bool {
            let system = process_system();
            let Ok(mut identities) = self.identities.lock() else {
                return false;
            };
            let mut roots: Vec<Pid> = identities
                .iter()
                .filter_map(|(pid, started)| {
                    system
                        .process(*pid)
                        .is_some_and(|process| process.start_time() == *started)
                        .then_some(*pid)
                })
                .collect();
            let current_descendants = sysinfo::get_current_pid()
                .ok()
                .map(|pid| descendant_order(&system, &[pid]))
                .unwrap_or_default();
            roots.extend(current_descendants.into_iter().filter(|pid| {
                system
                    .process(*pid)
                    .is_some_and(|process| self.baseline.get(pid) != Some(&process.start_time()))
            }));
            let termination_order = descendant_order(&system, &roots);
            for pid in &termination_order {
                if let Some(process) = system.process(*pid) {
                    identities
                        .entry(*pid)
                        .or_insert_with(|| process.start_time());
                }
            }
            drop(identities);

            for pid in &termination_order {
                if let Some(process) = system.process(*pid) {
                    let _ = process.kill();
                }
            }
            !termination_order.is_empty()
        }
    }

    fn process_system() -> System {
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::All, true);
        system
    }

    fn descendant_order(system: &System, roots: &[Pid]) -> Vec<Pid> {
        let children = system.processes().values().fold(
            HashMap::<Pid, Vec<Pid>>::new(),
            |mut grouped, process| {
                if let Some(parent) = process.parent() {
                    grouped.entry(parent).or_default().push(process.pid());
                }
                grouped
            },
        );
        let mut visited = HashSet::new();
        let mut order = Vec::new();
        for root in roots {
            let mut stack = vec![(*root, false)];
            while let Some((pid, expanded)) = stack.pop() {
                if expanded {
                    order.push(pid);
                    continue;
                }
                if !visited.insert(pid) {
                    continue;
                }
                stack.push((pid, true));
                if let Some(process_children) = children.get(&pid) {
                    stack.extend(process_children.iter().copied().map(|child| (child, false)));
                }
            }
        }
        order
    }
}

pub(super) use platform::SpawnRegistry;
