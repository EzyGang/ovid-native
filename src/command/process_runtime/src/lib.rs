#[cfg(windows)]
mod windows {
    use std::collections::{HashMap, HashSet};
    use std::mem;

    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        TerminateProcess,
    };

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct ProcessIdentity {
        pid: u32,
        created: u64,
    }

    pub fn current_pid() -> u32 {
        std::process::id()
    }

    pub fn descendants(root: u32) -> Vec<ProcessIdentity> {
        let Some(entries) = process_entries() else {
            return Vec::new();
        };
        let children = entries.into_iter().fold(
            HashMap::<u32, Vec<u32>>::new(),
            |mut grouped, (pid, parent)| {
                grouped.entry(parent).or_default().push(pid);
                grouped
            },
        );
        let mut visited = HashSet::from([root]);
        let mut order = Vec::new();
        let mut stack = children
            .get(&root)
            .into_iter()
            .flatten()
            .copied()
            .map(|pid| (pid, false))
            .collect::<Vec<_>>();
        while let Some((pid, expanded)) = stack.pop() {
            if expanded {
                if let Some(identity) = identity(pid) {
                    order.push(identity);
                }
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
        order
    }

    pub fn is_running(identity: ProcessIdentity) -> bool {
        current_creation_time(identity.pid) == Some(identity.created)
    }

    pub fn terminate(identity: ProcessIdentity) -> bool {
        let Some(handle) = ProcessHandle::open(identity.pid, PROCESS_REFERENCE_ACCESS) else {
            return false;
        };
        if creation_time(handle.raw) != Some(identity.created) {
            return false;
        }
        // SAFETY: The handle belongs to the same process identity and includes PROCESS_TERMINATE access.
        unsafe { TerminateProcess(handle.raw, 1) != 0 }
    }

    const PROCESS_REFERENCE_ACCESS: u32 = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE;

    struct ProcessHandle {
        raw: HANDLE,
    }

    impl ProcessHandle {
        fn open(pid: u32, access: u32) -> Option<Self> {
            // SAFETY: OpenProcess accepts a numeric process identifier. A null result is handled.
            let raw = unsafe { OpenProcess(access, 0, pid) };
            (!raw.is_null()).then_some(Self { raw })
        }
    }

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            // SAFETY: This handle was returned by OpenProcess and is closed exactly once.
            let _ = unsafe { CloseHandle(self.raw) };
        }
    }

    fn process_entries() -> Option<Vec<(u32, u32)>> {
        // SAFETY: The snapshot has no caller-owned pointer arguments. Invalid handles are rejected.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let snapshot = SnapshotHandle(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: mem::size_of::<PROCESSENTRY32W>().try_into().ok()?,
            ..unsafe { mem::zeroed() }
        };
        // SAFETY: entry has the required size and remains writable for the enumeration call.
        if unsafe { Process32FirstW(snapshot.0, &raw mut entry) } == 0 {
            return None;
        }
        let mut entries = Vec::new();
        loop {
            entries.push((entry.th32ProcessID, entry.th32ParentProcessID));
            // SAFETY: entry remains initialized with its required size for the next enumeration call.
            if unsafe { Process32NextW(snapshot.0, &raw mut entry) } == 0 {
                break;
            }
        }
        Some(entries)
    }

    struct SnapshotHandle(HANDLE);

    impl Drop for SnapshotHandle {
        fn drop(&mut self) {
            // SAFETY: This handle was returned by CreateToolhelp32Snapshot and is closed exactly once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn identity(pid: u32) -> Option<ProcessIdentity> {
        Some(ProcessIdentity {
            pid,
            created: current_creation_time(pid)?,
        })
    }

    fn current_creation_time(pid: u32) -> Option<u64> {
        let handle = ProcessHandle::open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        creation_time(handle.raw)
    }

    fn creation_time(handle: HANDLE) -> Option<u64> {
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: All pointers reference initialized FILETIME storage for the duration of this call.
        let result = unsafe {
            GetProcessTimes(
                handle,
                &raw mut created,
                &raw mut exited,
                &raw mut kernel,
                &raw mut user,
            )
        };
        if result == 0 {
            return None;
        }
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

#[cfg(windows)]
pub use windows::{ProcessIdentity, current_pid, descendants, is_running, terminate};
