//! Private daemon lifetime: cancellation, bounded ownership and actual worker joins.
//!
//! The admission mutex is never held while waiting for RPC ownership or I/O.
//! A claimant takes the RPC mutex first, then calls `while_running` for its
//! short claim mutation. Stop publishes under that same admission mutex,
//! releases it, interrupts sockets/writers, and only then wakes RPC waiters.

use std::collections::VecDeque;
use std::io;
use std::net::{Shutdown, TcpStream};
use std::ops::{Deref, DerefMut};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StopReason {
    AuthenticatedShutdown,
    Signal,
    Idle,
    StartupFailure,
    RecoveryFailure,
    FirefoxConnectionLost,
    WorkerReturned(&'static str),
    WorkerPanicked(&'static str),
    OwnerDropped,
}

/// Ordering is part of the 262 cancellation contract, not registration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum WakePhase {
    SocketAndWriter,
    Queue,
    Rpc,
}

type Wake = Arc<dyn Fn() + Send + Sync>;

struct WakeEntry {
    id: u64,
    phase: WakePhase,
    wake: Wake,
}

#[derive(Default)]
struct Admission {
    reason: Option<StopReason>,
    next_registration: u64,
    wakes: Vec<WakeEntry>,
}

#[derive(Default)]
pub(super) struct Cancellation {
    admission: Mutex<Admission>,
    stopped: Condvar,
    #[cfg(test)]
    connect_probe: Mutex<Option<ConnectPollProbe>>,
}

#[cfg(test)]
struct ConnectPollProbe {
    entered: std::sync::mpsc::Sender<()>,
    proceed: std::sync::mpsc::Receiver<()>,
    polled: std::sync::mpsc::Sender<Vec<usize>>,
}

impl Cancellation {
    /// `action` may only inspect/mutate already-locked admission state. It
    /// must not acquire the RPC mutex, wait, spawn, or perform I/O.
    pub(super) fn while_running<R>(&self, action: impl FnOnce() -> R) -> Result<R, StopReason> {
        let guard = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match guard.reason {
            Some(reason) => Err(reason),
            None => Ok(action()),
        }
    }

    pub(super) fn reason(&self) -> Option<StopReason> {
        self.admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reason
    }

    /// Publish once, then wake outside the admission gate. Callers must not
    /// hold RPC ownership or a writer lease when invoking this method.
    pub(super) fn request_stop(&self, reason: StopReason) -> bool {
        let mut wakes = {
            let mut guard = self
                .admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.reason.is_some() {
                return false;
            }
            guard.reason = Some(reason);
            self.stopped.notify_all();
            guard
                .wakes
                .iter()
                .map(|entry| (entry.phase, Arc::clone(&entry.wake)))
                .collect::<Vec<_>>()
        };
        wakes.sort_by_key(|(phase, _)| *phase);
        for (_, wake) in wakes {
            wake();
        }
        true
    }

    pub(super) fn register(
        self: &Arc<Self>,
        phase: WakePhase,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Registration {
        let wake: Wake = Arc::new(wake);
        let id = {
            let mut guard = self
                .admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.reason.is_some() {
                None
            } else {
                let id = guard.next_registration;
                guard.next_registration += 1;
                guard.wakes.push(WakeEntry {
                    id,
                    phase,
                    wake: Arc::clone(&wake),
                });
                Some(id)
            }
        };
        if id.is_none() {
            wake();
        }
        Registration {
            owner: Arc::downgrade(self),
            id,
        }
    }

    pub(super) fn register_socket(
        self: &Arc<Self>,
        socket: &TcpStream,
    ) -> io::Result<Registration> {
        let interrupt = socket.try_clone()?;
        Ok(self.register(WakePhase::SocketAndWriter, move || {
            let _ = interrupt.shutdown(Shutdown::Both);
        }))
    }

    /// Interrupt an existing retry/accept interval without changing its budget.
    pub(super) fn wait_until(&self, deadline: Instant) -> bool {
        let mut guard = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while guard.reason.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            guard = self
                .stopped
                .wait_timeout(guard, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        true
    }
}

pub(super) struct Registration {
    owner: Weak<Cancellation>,
    id: Option<u64>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let (Some(owner), Some(id)) = (self.owner.upgrade(), self.id) {
            owner
                .admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .wakes
                .retain(|entry| entry.id != id);
        }
    }
}

struct QueueState<T> {
    items: VecDeque<T>,
    closed: bool,
    #[cfg(test)]
    send_waiters: usize,
    #[cfg(test)]
    recv_waiters: usize,
}

/// Closing cancels pending operations; it never changes normal FIFO/capacity.
pub(super) struct BoundedQueue<T> {
    capacity: usize,
    state: Mutex<QueueState<T>>,
    readable: Condvar,
    writable: Condvar,
    #[cfg(test)]
    blocked: Condvar,
}

impl<T> BoundedQueue<T> {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(QueueState {
                items: VecDeque::new(),
                closed: false,
                #[cfg(test)]
                send_waiters: 0,
                #[cfg(test)]
                recv_waiters: 0,
            }),
            readable: Condvar::new(),
            writable: Condvar::new(),
            #[cfg(test)]
            blocked: Condvar::new(),
        }
    }

    pub(super) fn send(&self, value: T) -> Result<(), T> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while guard.items.len() >= self.capacity && !guard.closed {
            #[cfg(test)]
            {
                guard.send_waiters += 1;
                self.blocked.notify_all();
            }
            guard = self
                .writable
                .wait(guard)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            #[cfg(test)]
            {
                guard.send_waiters -= 1;
            }
        }
        if guard.closed {
            return Err(value);
        }
        guard.items.push_back(value);
        self.readable.notify_one();
        Ok(())
    }

    pub(super) fn recv(&self) -> Option<T> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if guard.closed {
                return None;
            }
            if let Some(value) = guard.items.pop_front() {
                self.writable.notify_one();
                return Some(value);
            }
            #[cfg(test)]
            {
                guard.recv_waiters += 1;
                self.blocked.notify_all();
            }
            guard = self
                .readable
                .wait(guard)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            #[cfg(test)]
            {
                guard.recv_waiters -= 1;
            }
        }
    }

    pub(super) fn try_send(&self, value: T) -> Result<(), T> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.closed || guard.items.len() >= self.capacity {
            return Err(value);
        }
        guard.items.push_back(value);
        self.readable.notify_one();
        Ok(())
    }

    pub(super) fn try_recv(&self) -> Option<T> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.closed {
            return None;
        }
        let item = guard.items.pop_front();
        if item.is_some() {
            self.writable.notify_one();
        }
        item
    }

    pub(super) fn recv_until(&self, deadline: Instant) -> Option<T> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if guard.closed {
                return None;
            }
            if let Some(value) = guard.items.pop_front() {
                self.writable.notify_one();
                return Some(value);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            guard = self
                .readable
                .wait_timeout(guard, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }

    #[cfg(test)]
    pub(super) fn wait_for_blocked(&self, sender: bool, deadline: Instant) -> bool {
        let mut guard = self.state.lock().expect("queue observation");
        loop {
            if if sender {
                guard.send_waiters > 0
            } else {
                guard.recv_waiters > 0
            } {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            guard = self
                .blocked
                .wait_timeout(guard, remaining)
                .expect("queue waiter observation")
                .0;
        }
    }

    pub(super) fn close(&self) {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.closed = true;
        self.readable.notify_all();
        self.writable.notify_all();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LeaseError {
    Cancelled,
    Deadline,
}

struct SlotState<T> {
    value: Option<T>,
    closed: bool,
    #[cfg(test)]
    waiters: usize,
}

/// The mutex protects ownership only; the lease holds the actual writer while
/// performing I/O. Cancellation never needs that lease to wake its waiters.
pub(super) struct WriterSlot<T> {
    state: Mutex<SlotState<T>>,
    available: Condvar,
    #[cfg(test)]
    blocked: Condvar,
}

impl<T> WriterSlot<T> {
    pub(super) fn new(value: T) -> Self {
        Self {
            state: Mutex::new(SlotState {
                value: Some(value),
                closed: false,
                #[cfg(test)]
                waiters: 0,
            }),
            available: Condvar::new(),
            #[cfg(test)]
            blocked: Condvar::new(),
        }
    }

    /// Preserve ordinary Firefox writer backpressure without inventing a new
    /// lease timeout. Stop closes the slot and wakes this wait independently.
    pub(super) fn acquire_cancellable(&self) -> Option<WriterLease<'_, T>> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if guard.closed {
                return None;
            }
            if let Some(value) = guard.value.take() {
                return Some(WriterLease {
                    slot: self,
                    value: Some(value),
                });
            }
            #[cfg(test)]
            {
                guard.waiters += 1;
                self.blocked.notify_all();
            }
            guard = self
                .available
                .wait(guard)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            #[cfg(test)]
            {
                guard.waiters -= 1;
            }
        }
    }

    pub(super) fn acquire(&self, deadline: Instant) -> Result<WriterLease<'_, T>, LeaseError> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if guard.closed {
                return Err(LeaseError::Cancelled);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(LeaseError::Deadline);
            }
            if let Some(value) = guard.value.take() {
                return Ok(WriterLease {
                    slot: self,
                    value: Some(value),
                });
            }
            #[cfg(test)]
            {
                guard.waiters += 1;
                self.blocked.notify_all();
            }
            let (next, _) = self
                .available
                .wait_timeout(guard, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard = next;
            #[cfg(test)]
            {
                guard.waiters -= 1;
            }
        }
    }

    #[cfg(test)]
    pub(super) fn wait_for_blocked(&self, deadline: Instant) -> bool {
        let mut guard = self.state.lock().expect("writer observation");
        loop {
            if guard.waiters > 0 {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            guard = self
                .blocked
                .wait_timeout(guard, remaining)
                .expect("writer waiter observation")
                .0;
        }
    }

    pub(super) fn close(&self) {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.closed = true;
        self.available.notify_all();
    }
}

pub(super) struct WriterLease<'a, T> {
    slot: &'a WriterSlot<T>,
    value: Option<T>,
}

impl<T> Deref for WriterLease<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        match self.value.as_ref() {
            Some(value) => value,
            None => unreachable!("a live writer lease owns its writer"),
        }
    }
}

impl<T> DerefMut for WriterLease<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        match self.value.as_mut() {
            Some(value) => value,
            None => unreachable!("a live writer lease owns its writer"),
        }
    }
}

impl<T> Drop for WriterLease<'_, T> {
    fn drop(&mut self) {
        let mut guard = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.value = self.value.take();
        self.slot.available.notify_one();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkerPolicy {
    Required,
    Optional,
    Client,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WorkerOutcome {
    Stopped,
    ClientReturned,
    OptionalReturned,
    UnexpectedReturn,
    Panicked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JoinRecord {
    pub(super) name: &'static str,
    pub(super) outcome: WorkerOutcome,
    /// False only if the supervision epilogue itself escaped with a panic.
    pub(super) supervision_returned: bool,
}

pub(super) struct WorkerOwner {
    cancellation: Arc<Cancellation>,
    handles: Vec<(&'static str, JoinHandle<WorkerOutcome>)>,
    joined: Vec<JoinRecord>,
    saw_failure: bool,
    joined_count: u64,
    #[cfg(test)]
    fail_next_spawn: bool,
    #[cfg(test)]
    join_observer: Option<std::sync::mpsc::Sender<JoinRecord>>,
}

impl WorkerOwner {
    pub(super) fn new(cancellation: Arc<Cancellation>) -> Self {
        Self {
            cancellation,
            handles: Vec::new(),
            joined: Vec::new(),
            saw_failure: false,
            joined_count: 0,
            #[cfg(test)]
            fail_next_spawn: false,
            #[cfg(test)]
            join_observer: None,
        }
    }

    pub(super) fn spawn(
        &mut self,
        name: &'static str,
        policy: WorkerPolicy,
        body: impl FnOnce() + Send + 'static,
    ) -> io::Result<()> {
        self.cancellation
            .while_running(|| ())
            .map_err(|_| io::Error::new(io::ErrorKind::Interrupted, "daemon admission closed"))?;
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_spawn) {
            return Err(io::Error::other("controlled worker acquisition failure"));
        }
        let cancellation = Arc::clone(&self.cancellation);
        let handle = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                if cancellation.reason().is_some() {
                    return WorkerOutcome::Stopped;
                }
                if catch_unwind(AssertUnwindSafe(body)).is_err() {
                    cancellation.request_stop(StopReason::WorkerPanicked(name));
                    return WorkerOutcome::Panicked;
                }
                if cancellation.reason().is_some() {
                    return WorkerOutcome::Stopped;
                }
                match policy {
                    WorkerPolicy::Required => {
                        cancellation.request_stop(StopReason::WorkerReturned(name));
                        WorkerOutcome::UnexpectedReturn
                    }
                    WorkerPolicy::Optional => WorkerOutcome::OptionalReturned,
                    WorkerPolicy::Client => WorkerOutcome::ClientReturned,
                }
            })?;
        self.handles.push((name, handle));
        Ok(())
    }

    pub(super) fn shutdown_and_join(&mut self, reason: StopReason) -> &[JoinRecord] {
        self.cancellation.request_stop(reason);
        self.join_acquired()
    }

    pub(super) fn join_acquired(&mut self) -> &[JoinRecord] {
        while !self.handles.is_empty() {
            self.join_one(0);
        }
        &self.joined
    }

    fn join_one(&mut self, index: usize) {
        let (name, handle) = self.handles.remove(index);
        let (outcome, supervision_returned) = match handle.join() {
            Ok(outcome) => (outcome, true),
            Err(_) => (WorkerOutcome::Panicked, false),
        };
        self.joined_count += 1;
        self.saw_failure |= !supervision_returned
            || matches!(
                outcome,
                WorkerOutcome::Panicked | WorkerOutcome::UnexpectedReturn
            );
        tracing::info!(worker = name, outcome = ?outcome, supervision_returned, joined_count = self.joined_count, "daemon: actual worker join");
        let record = JoinRecord {
            name,
            outcome,
            supervision_returned,
        };
        #[cfg(test)]
        if let Some(observer) = &self.join_observer {
            let _ = observer.send(record.clone());
        }
        self.joined.push(record);
    }

    pub(super) fn reap_finished(&mut self) {
        let mut index = 0;
        while index < self.handles.len() {
            if self.handles[index].1.is_finished() {
                self.join_one(index);
                // Its actual outcome was emitted above; retain aggregate failure
                // and count rather than accumulating every historical client.
                self.joined.pop();
            } else {
                index += 1;
            }
        }
    }

    #[cfg(test)]
    pub(super) fn observe_joins(&mut self, sender: std::sync::mpsc::Sender<JoinRecord>) {
        self.join_observer = Some(sender);
    }

    #[cfg(test)]
    pub(super) fn fail_next_acquisition(&mut self) {
        self.fail_next_spawn = true;
    }

    pub(super) fn completed_successfully(&self) -> bool {
        self.handles.is_empty()
            && !self.saw_failure
            && self.joined.iter().all(|record| {
                record.supervision_returned
                    && !matches!(
                        record.outcome,
                        WorkerOutcome::Panicked | WorkerOutcome::UnexpectedReturn
                    )
            })
            && matches!(
                self.cancellation.reason(),
                Some(StopReason::AuthenticatedShutdown | StopReason::Signal | StopReason::Idle)
            )
    }
}

impl Drop for WorkerOwner {
    fn drop(&mut self) {
        if !self.handles.is_empty() {
            for record in self.shutdown_and_join(StopReason::OwnerDropped) {
                tracing::debug!(worker = record.name, outcome = ?record.outcome, "daemon: joined worker during owner cleanup");
            }
        }
    }
}

#[cfg(test)]
mod tests;

/// Acquire the optional connection to the primary connection's actual peer.
/// DNS belongs to initial primary acquisition before owned workers exist; no
/// resolver thread or blocking connect is hidden inside this owned worker.
pub(super) fn connect_cancellable(
    peer: std::net::SocketAddr,
    cancellation: &Arc<Cancellation>,
    deadline: Instant,
) -> io::Result<TcpStream> {
    const SOCKET: mio::Token = mio::Token(0);
    const CANCEL: mio::Token = mio::Token(1);
    let mut poll = mio::Poll::new()?;
    let wake = Arc::new(mio::Waker::new(poll.registry(), CANCEL)?);
    let _registration = cancellation.register(WakePhase::SocketAndWriter, move || {
        let _ = wake.wake();
    });
    if cancellation.reason().is_some() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    let mut socket = mio::net::TcpStream::connect(peer)?;
    poll.registry()
        .register(&mut socket, SOCKET, mio::Interest::WRITABLE)?;
    let mut events = mio::Events::with_capacity(4);
    loop {
        if cancellation.reason().is_some() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        // The optional test barrier controls application scheduling only; it
        // neither asserts nor changes the kernel's TCP connection state.
        #[cfg(test)]
        let probe = cancellation
            .connect_probe
            .lock()
            .expect("connector probe")
            .take();
        #[cfg(test)]
        if let Some(probe) = &probe {
            let _ = probe.entered.send(());
            let _ = probe
                .proceed
                .recv_timeout(std::time::Duration::from_secs(5));
        }
        match poll.poll(&mut events, Some(remaining)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
        #[cfg(test)]
        if let Some(probe) = probe {
            let _ = probe
                .polled
                .send(events.iter().map(|event| event.token().0).collect());
        }
        if cancellation.reason().is_some() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        for event in &events {
            if event.token() == SOCKET {
                if let Some(error) = socket.take_error()? {
                    return Err(error);
                }
                match socket.peer_addr() {
                    Ok(_) => {
                        poll.registry().deregister(&mut socket)?;
                        let stream = TcpStream::from(socket);
                        stream.set_nonblocking(false)?;
                        return Ok(stream);
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotConnected | io::ErrorKind::WouldBlock
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
}
