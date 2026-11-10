// SPDX-License-Identifier: GPL-3.0-or-later

//! Process measurement for the task manager (PRD-024, ADR-0093).
//!
//! **Why this lives in `porecatu-pty` and not in a monitoring crate:**
//! the membership criterion ("this process belongs to this tab") and the Job
//! Object that backs it live in `job.rs`, next to `sysinfo`. Measuring in
//! another crate would force exposing the Job (leaking `win32job`) or
//! duplicating the union, and both break RF-24.14 (a single criterion). Here,
//! [`Sampler`] calls `ProcessGroup::members`, the same function used by tab
//! closing and by the count (ADR-0093 §1 and §2).
//!
//! The crate knows nothing about windows, tabs or `winit`: the pane key is an
//! opaque `K` from the caller, and the result comes out through a callback
//! that runs on the `porecatu-sampler` thread. No `sysinfo` type crosses the
//! boundary -- the public types all belong to this module.
//!
//! The thread is **one per process** and lives between requests because a
//! process's CPU is the **difference** between two updates of the same
//! `sysinfo::System` (ADR-0093 §4). The pace (1 Hz) belongs to the caller;
//! this thread only serves requests and ends when the channel closes.

use crate::job::{ProcessGroup, descendants_of};
use std::collections::HashSet;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// Request for the sampling thread (ADR-0093 §5 and §7).
pub enum SamplerRequest<K> {
    /// Measure the listed panes.
    Sample(SampleRequest<K>),
    /// Terminate a process chosen in the list, with no reply.
    Kill(KillRequest),
}

/// Sample request: one item per pane with a local process.
pub struct SampleRequest<K> {
    /// Request number; comes back in [`SampleResult::seq`] so the caller can
    /// discard stale replies.
    pub seq: u64,
    /// The caller's opaque key and the pane's process group.
    pub roots: Vec<(K, ProcessGroup)>,
}

/// A live process of a tree, as measured in a sample.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessStat {
    pub pid: u32,
    pub parent: Option<u32>,
    /// Seconds since the epoch, from `sysinfo`. Together with the PID, it
    /// identifies the process (a reused PID has a different start time).
    pub start_time: u64,
    pub name: String,
    /// `0.0..=1.0` of the whole machine; `None` on the first sample of a
    /// `System` (CPU is the difference between two updates, RF-24.9).
    pub cpu: Option<f32>,
    /// Resident bytes (working set on Windows).
    pub memory: u64,
    /// Bytes read plus written per second.
    pub disk_per_sec: f64,
}

/// Reply to a [`SampleRequest`].
#[derive(Debug)]
pub struct SampleResult<K> {
    pub seq: u64,
    /// Instant of the `System` update, for the time axis.
    pub taken_at: Instant,
    /// For each key in the request, the live members of that pane's tree.
    /// A PID already seen in another pane of the same request stays only in
    /// the first one.
    pub panes: Vec<(K, Vec<ProcessStat>)>,
}

/// Request to terminate a process (ADR-0093 §7).
#[derive(Debug, Clone)]
pub struct KillRequest {
    pub group: ProcessGroup,
    pub pid: u32,
    /// The `start_time` of the row the user chose.
    pub start_time: u64,
}

/// The sampling thread has already ended (the `System` could not be created,
/// or the loop finished): the request was not delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplerGone;

/// Handle of the `porecatu-sampler` thread. Dropping the `Sampler` closes the
/// channel, and the thread ends on its own and drops the `System` (ADR-0093
/// §4).
pub struct Sampler<K> {
    tx: mpsc::Sender<SamplerRequest<K>>,
    handle: JoinHandle<()>,
}

impl<K: Send + 'static> Sampler<K> {
    /// Starts the `porecatu-sampler` thread. `on_result` runs **on that
    /// thread**, once per [`SamplerRequest::Sample`].
    pub fn spawn(on_result: impl Fn(SampleResult<K>) + Send + 'static) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel::<SamplerRequest<K>>();
        let handle = std::thread::Builder::new()
            .name("porecatu-sampler".to_string())
            .spawn(move || {
                // `System::new` is fallible in sysinfo 0.40: without it the
                // thread ends, `rx` is dropped and `send` starts returning an
                // error.
                let Some(mut state) = SamplerState::new() else {
                    return;
                };
                while let Ok(request) = rx.recv() {
                    match request {
                        SamplerRequest::Sample(sample) => on_result(state.sample(sample)),
                        SamplerRequest::Kill(kill) => state.kill(&kill),
                    }
                }
            })?;
        Ok(Self { tx, handle })
    }

    /// Does not block. `Err` if the thread no longer exists.
    pub fn send(&self, request: SamplerRequest<K>) -> Result<(), SamplerGone> {
        self.tx.send(request).map_err(|_| SamplerGone)
    }

    /// Drops the channel (the thread ends once it drains it) and returns the
    /// thread handle. Tests `join` it; the UI just drops the `Sampler`.
    pub fn close(self) -> JoinHandle<()> {
        self.handle
    }
}

/// Fallback from ADR-0093 §7: a single-use thread, with a fresh `System`, for
/// a confirmed termination when the sampling thread no longer exists.
/// Does not block the caller.
pub fn kill_detached(request: KillRequest) {
    let spawned = std::thread::Builder::new()
        .name("porecatu-kill".to_string())
        .spawn(move || {
            if let Some(mut state) = SamplerState::new() {
                state.kill(&request);
            }
        });
    // No thread, no termination: the request is best-effort and the
    // process, if it stays alive, reappears in the list.
    drop(spawned);
}

/// The `System` that survives between requests, plus what the math needs.
struct SamplerState {
    system: System,
    logical_cpus: usize,
    /// Instant of the last **sample** refresh (CPU and disk). `None` until
    /// the first one.
    last_refresh: Option<Instant>,
}

impl SamplerState {
    fn new() -> Option<Self> {
        let mut system = System::new().ok()?;
        system.refresh_cpu_list(CpuRefreshKind::nothing());
        let mut logical_cpus = system.cpus().len();
        if logical_cpus == 0 {
            logical_cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
        }
        Some(Self {
            system,
            logical_cpus,
            last_refresh: None,
        })
    }

    /// ADR-0093 §5.
    fn sample<K>(&mut self, request: SampleRequest<K>) -> SampleResult<K> {
        let first = self.last_refresh.is_none();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_disk_usage(),
        );
        let taken_at = Instant::now();
        let elapsed = self
            .last_refresh
            .map_or(Duration::ZERO, |previous| taken_at.duration_since(previous));
        self.last_refresh = Some(taken_at);

        let mut seen: HashSet<u32> = HashSet::new();
        let mut panes = Vec::with_capacity(request.roots.len());
        for (key, group) in request.roots {
            let mut pids: Vec<u32> = group.members(&self.system).into_iter().collect();
            // Stable order across samples; the UI builds the tree from `parent`.
            pids.sort_unstable();
            let mut stats = Vec::with_capacity(pids.len());
            for pid in pids {
                if !seen.insert(pid) {
                    continue;
                }
                let Some(process) = self.system.process(Pid::from_u32(pid)) else {
                    continue;
                };
                let usage = process.disk_usage();
                stats.push(ProcessStat {
                    pid,
                    parent: process.parent().map(Pid::as_u32),
                    start_time: process.start_time(),
                    name: process.name().to_string_lossy().into_owned(),
                    cpu: if first {
                        None
                    } else {
                        Some(normalize_cpu(process.cpu_usage(), self.logical_cpus))
                    },
                    memory: process.memory(),
                    disk_per_sec: disk_rate(
                        usage.read_bytes.saturating_add(usage.written_bytes),
                        elapsed,
                    ),
                });
            }
            panes.push((key, stats));
        }
        SampleResult {
            seq: request.seq,
            taken_at,
            panes,
        }
    }

    /// ADR-0093 §7: checks identity and kills, leaves first.
    fn kill(&mut self, request: &KillRequest) {
        // Process list only: parent and start time always come, CPU and disk don't.
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        let pid = Pid::from_u32(request.pid);
        let current_start = self.system.process(pid).map(|p| p.start_time());
        let members = request.group.members(&self.system);
        if !kill_allowed(
            request.pid,
            request.start_time,
            request.group.root_pid(),
            current_start,
            &members,
        ) {
            return;
        }
        // `descendants_of` puts every parent before its children: reversed,
        // that is leaves first, and no orphaned child is re-parented midway.
        let mut victims = descendants_of(&self.system, pid);
        victims.reverse();
        victims.push(pid);
        for victim in victims {
            if let Some(process) = self.system.process(victim) {
                let _ = process.kill();
            }
        }
    }
}

/// `sysinfo`'s `cpu_usage()` is in % of **one** core; the output is the
/// fraction of the whole machine (`0.0..=1.0`). Zero CPUs counts as one, to
/// avoid dividing by zero.
fn normalize_cpu(raw_percent_of_one_core: f32, logical_cpus: usize) -> f32 {
    if !raw_percent_of_one_core.is_finite() {
        return 0.0;
    }
    (raw_percent_of_one_core / 100.0 / logical_cpus.max(1) as f32).clamp(0.0, 1.0)
}

/// Bytes per second, dividing by the **real** time between the two updates.
/// Zero if no time has passed (first sample).
fn disk_rate(bytes: u64, elapsed: Duration) -> f64 {
    let seconds = elapsed.as_secs_f64();
    if seconds <= 0.0 {
        return 0.0;
    }
    bytes as f64 / seconds
}

/// The decision to terminate (ADR-0093 §7, steps 2 and 3): the PID still
/// exists with the same `start_time` as the chosen row (`current_start`), is
/// a member of the pane's tree and is **not** the root (defense in depth for
/// RF-24.22).
fn kill_allowed(
    pid: u32,
    start_time: u64,
    root_pid: u32,
    current_start: Option<u64>,
    members: &HashSet<u32>,
) -> bool {
    current_start == Some(start_time) && pid != root_pid && members.contains(&pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_is_a_fraction_of_the_whole_machine() {
        // 200% of one core on 8 logical CPUs.
        assert!((normalize_cpu(200.0, 8) - 0.25).abs() < 1e-6);
        assert_eq!(normalize_cpu(0.0, 8), 0.0);
    }

    #[test]
    fn cpu_is_clamped_to_one() {
        assert_eq!(normalize_cpu(1600.0, 8), 1.0);
        assert_eq!(normalize_cpu(100.0, 1), 1.0);
        assert_eq!(normalize_cpu(-5.0, 4), 0.0);
    }

    #[test]
    fn cpu_survives_zero_cpus_and_non_finite_input() {
        assert_eq!(normalize_cpu(50.0, 0), 0.5);
        assert_eq!(normalize_cpu(f32::NAN, 4), 0.0);
        assert_eq!(normalize_cpu(f32::INFINITY, 4), 0.0);
    }

    #[test]
    fn disk_rate_divides_by_the_real_elapsed_time() {
        assert_eq!(disk_rate(1000, Duration::from_millis(500)), 2000.0);
        assert_eq!(disk_rate(3000, Duration::from_secs(3)), 1000.0);
        assert_eq!(disk_rate(0, Duration::from_secs(1)), 0.0);
    }

    #[test]
    fn disk_rate_is_zero_without_elapsed_time() {
        assert_eq!(disk_rate(4096, Duration::ZERO), 0.0);
    }

    fn members(pids: &[u32]) -> HashSet<u32> {
        pids.iter().copied().collect()
    }

    #[test]
    fn kill_is_allowed_for_a_matching_member() {
        assert!(kill_allowed(20, 1000, 10, Some(1000), &members(&[10, 20])));
    }

    #[test]
    fn kill_is_refused_when_the_start_time_differs() {
        assert!(!kill_allowed(20, 1000, 10, Some(1001), &members(&[10, 20])));
    }

    #[test]
    fn kill_is_refused_when_the_process_is_gone() {
        assert!(!kill_allowed(20, 1000, 10, None, &members(&[10, 20])));
    }

    #[test]
    fn kill_is_refused_for_the_root() {
        assert!(!kill_allowed(10, 1000, 10, Some(1000), &members(&[10, 20])));
    }

    #[test]
    fn kill_is_refused_outside_the_members() {
        assert!(!kill_allowed(30, 1000, 10, Some(1000), &members(&[10, 20])));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::job::test_support::spawn_interactive_and_run;
    use crate::spawn::PtyHandle;
    use std::sync::mpsc::RecvTimeoutError;

    const WAIT: Duration = Duration::from_secs(30);

    /// The three PIDs of the `cmd.exe` (root) -> `cmd /c` -> `ping` tree.
    struct Tree {
        handle: PtyHandle,
        group: ProcessGroup,
        root: u32,
        child: u32,
        grandchild: u32,
    }

    fn fresh_system() -> System {
        let mut system = System::new().expect("sysinfo");
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        system
    }

    /// Starts the three-level tree and waits, with a generous deadline (no
    /// fixed sleep), for all three to be alive.
    fn spawn_tree() -> Tree {
        let (handle, group) =
            spawn_interactive_and_run("cmd.exe", None, ">", "cmd /c ping -n 30 127.0.0.1");
        let root = group.root_pid();
        let deadline = Instant::now() + WAIT;
        loop {
            let system = fresh_system();
            let child = system
                .processes()
                .iter()
                .find(|(_, p)| p.parent() == Some(Pid::from_u32(root)))
                .map(|(pid, _)| *pid);
            if let Some(child) = child {
                let grandchild = system
                    .processes()
                    .iter()
                    .find(|(_, p)| p.parent() == Some(child))
                    .map(|(pid, _)| *pid);
                if let Some(grandchild) = grandchild {
                    return Tree {
                        handle,
                        group,
                        root,
                        child: child.as_u32(),
                        grandchild: grandchild.as_u32(),
                    };
                }
            }
            assert!(
                Instant::now() < deadline,
                "the root -> child -> grandchild tree did not come up in time"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn alive(pid: u32) -> bool {
        fresh_system().process(Pid::from_u32(pid)).is_some()
    }

    fn wait_dead(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive(pid) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        true
    }

    fn start_time_of(pid: u32) -> u64 {
        fresh_system()
            .process(Pid::from_u32(pid))
            .expect("live process")
            .start_time()
    }

    fn sampler_with_channel() -> (
        Sampler<&'static str>,
        mpsc::Receiver<SampleResult<&'static str>>,
    ) {
        let (tx, rx) = mpsc::channel();
        let sampler = Sampler::spawn(move |result| {
            let _ = tx.send(result);
        })
        .expect("sampler thread");
        (sampler, rx)
    }

    fn request_sample(
        sampler: &Sampler<&'static str>,
        rx: &mpsc::Receiver<SampleResult<&'static str>>,
        seq: u64,
        group: &ProcessGroup,
    ) -> SampleResult<&'static str> {
        sampler
            .send(SamplerRequest::Sample(SampleRequest {
                seq,
                roots: vec![("pane", group.clone())],
            }))
            .expect("sampler alive");
        match rx.recv_timeout(WAIT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => panic!("sample did not arrive in time"),
            Err(RecvTimeoutError::Disconnected) => panic!("sampler died"),
        }
    }

    fn cleanup(tree: Tree) {
        let Tree {
            mut handle, group, ..
        } = tree;
        group.kill_tree();
        let _ = handle.kill();
    }

    #[test]
    fn members_returns_the_whole_tree() {
        let tree = spawn_tree();
        let system = fresh_system();
        let members = tree.group.members(&system);
        assert!(members.contains(&tree.root), "root missing: {members:?}");
        assert!(members.contains(&tree.child), "child missing: {members:?}");
        assert!(
            members.contains(&tree.grandchild),
            "grandchild missing: {members:?}"
        );
        cleanup(tree);
    }

    #[test]
    fn a_sample_carries_the_three_processes_and_cpu_needs_two_updates() {
        let tree = spawn_tree();
        let (sampler, rx) = sampler_with_channel();

        let first = request_sample(&sampler, &rx, 1, &tree.group);
        assert_eq!(first.seq, 1);
        let (key, stats) = &first.panes[0];
        assert_eq!(*key, "pane");
        for pid in [tree.root, tree.child, tree.grandchild] {
            let stat = stats
                .iter()
                .find(|s| s.pid == pid)
                .unwrap_or_else(|| panic!("pid {pid} missing from the sample: {stats:?}"));
            assert!(stat.memory > 0, "zero memory for {stat:?}");
            assert_eq!(stat.cpu, None, "the first sample has no CPU: {stat:?}");
            assert_eq!(stat.disk_per_sec, 0.0);
            assert!(!stat.name.is_empty());
        }
        let child = stats.iter().find(|s| s.pid == tree.child).unwrap();
        assert_eq!(child.parent, Some(tree.root));
        assert_eq!(child.start_time, start_time_of(tree.child));

        std::thread::sleep(Duration::from_millis(300));
        let second = request_sample(&sampler, &rx, 2, &tree.group);
        assert_eq!(second.seq, 2);
        assert!(second.taken_at > first.taken_at);
        for stat in &second.panes[0].1 {
            let cpu = stat
                .cpu
                .unwrap_or_else(|| panic!("the second sample has CPU: {stat:?}"));
            assert!((0.0..=1.0).contains(&cpu), "CPU out of range: {stat:?}");
            assert!(stat.disk_per_sec >= 0.0);
        }

        sampler.close().join().expect("the thread ends");
        cleanup(tree);
    }

    #[test]
    fn a_pid_seen_in_another_pane_stays_in_the_first_one() {
        let tree = spawn_tree();
        let mut state = SamplerState::new().expect("sysinfo");
        let result = state.sample(SampleRequest {
            seq: 7,
            roots: vec![("a", tree.group.clone()), ("b", tree.group.clone())],
        });
        assert!(!result.panes[0].1.is_empty());
        assert!(result.panes[1].1.is_empty());
        cleanup(tree);
    }

    #[test]
    fn kill_with_the_wrong_start_time_does_nothing() {
        let tree = spawn_tree();
        let (sampler, _rx) = sampler_with_channel();
        let wrong = start_time_of(tree.child).wrapping_add(1);
        sampler
            .send(SamplerRequest::Kill(KillRequest {
                group: tree.group.clone(),
                pid: tree.child,
                start_time: wrong,
            }))
            .expect("sampler alive");
        // The queue is ordered: after this `close`+`join` the request has been handled.
        sampler.close().join().expect("the thread ends");
        assert!(alive(tree.child));
        assert!(alive(tree.grandchild));
        cleanup(tree);
    }

    #[test]
    fn kill_never_takes_the_root() {
        let mut tree = spawn_tree();
        let (sampler, _rx) = sampler_with_channel();
        sampler
            .send(SamplerRequest::Kill(KillRequest {
                group: tree.group.clone(),
                pid: tree.root,
                start_time: start_time_of(tree.root),
            }))
            .expect("sampler alive");
        sampler.close().join().expect("the thread ends");
        assert!(alive(tree.root));
        assert!(tree.handle.try_wait().ok().flatten().is_none());
        cleanup(tree);
    }

    #[test]
    fn kill_takes_the_child_and_the_grandchild_but_not_the_root() {
        let mut tree = spawn_tree();
        let (sampler, _rx) = sampler_with_channel();
        sampler
            .send(SamplerRequest::Kill(KillRequest {
                group: tree.group.clone(),
                pid: tree.child,
                start_time: start_time_of(tree.child),
            }))
            .expect("sampler alive");
        assert!(wait_dead(tree.child), "the child should have died");
        assert!(
            wait_dead(tree.grandchild),
            "the grandchild should have died"
        );
        assert!(alive(tree.root), "the root must stay alive");
        assert!(tree.handle.try_wait().ok().flatten().is_none());
        sampler.close().join().expect("the thread ends");
        cleanup(tree);
    }

    #[test]
    fn kill_detached_takes_the_child_too() {
        let tree = spawn_tree();
        kill_detached(KillRequest {
            group: tree.group.clone(),
            pid: tree.child,
            start_time: start_time_of(tree.child),
        });
        assert!(wait_dead(tree.child));
        assert!(wait_dead(tree.grandchild));
        assert!(alive(tree.root));
        cleanup(tree);
    }

    #[test]
    fn closing_the_sampler_ends_its_thread() {
        let (sampler, _rx) = sampler_with_channel();
        sampler.close().join().expect("the thread ends");
    }

    #[test]
    fn send_fails_once_the_thread_is_gone() {
        let (tx, rx) = mpsc::channel::<SamplerRequest<()>>();
        // A `Sampler` without a thread: the receiver is already dropped.
        drop(rx);
        let sampler = Sampler {
            tx,
            handle: std::thread::spawn(|| {}),
        };
        let sent = sampler.send(SamplerRequest::Sample(SampleRequest {
            seq: 0,
            roots: Vec::new(),
        }));
        assert_eq!(sent.err(), Some(SamplerGone));
    }

    /// Measures one sample (the `System` refresh and building the
    /// `ProcessStat`s) for the record in `docs/descobertas.md` (ADR-0093,
    /// "Riscos"). Does not assert a ceiling: it only prints.
    #[test]
    fn measure_the_cost_of_one_sample() {
        let tree = spawn_tree();
        let mut state = SamplerState::new().expect("sysinfo");
        let processes = {
            let system = fresh_system();
            system.processes().len()
        };
        let mut times = Vec::new();
        for seq in 0..11 {
            let started = Instant::now();
            let result = state.sample(SampleRequest {
                seq,
                roots: vec![("pane", tree.group.clone())],
            });
            times.push(started.elapsed());
            assert!(result.panes[0].1.len() >= 3);
            std::thread::sleep(Duration::from_millis(200));
        }
        // The first (cold) one stays out of the summary of the following samples.
        let cold = times[0];
        let mut warm = times[1..].to_vec();
        warm.sort();
        eprintln!(
            "sample: {processes} processes on the system, {} logical CPUs; \
             cold {cold:?}; warm min {:?} median {:?} max {:?}",
            state.logical_cpus,
            warm[0],
            warm[warm.len() / 2],
            warm[warm.len() - 1],
        );
        cleanup(tree);
    }
}
