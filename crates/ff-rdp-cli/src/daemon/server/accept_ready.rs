//! Listener readiness and cancellation; the owner still checks signal/idle state.
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use super::{Cancellation, Registration, WakePhase};

const LISTENER: mio::Token = mio::Token(0);
const CANCEL: mio::Token = mio::Token(1);
// Keep the existing signal/idle housekeeping ceiling, but wake on queued I/O.
const HOUSEKEEPING_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Wake {
    Readable,
    Cancelled,
    Housekeeping,
}

pub(super) struct ReadyListener {
    // Unregister the callback before dropping the poll/listener. A concurrently
    // extracted callback retains its own Arc<Waker>; wake failure is harmless.
    _registration: Registration,
    listener: mio::net::TcpListener,
    poll: mio::Poll,
    events: mio::Events,
    cancellation: Arc<Cancellation>,
    #[cfg(test)]
    probe: Option<PollProbe>,
}

impl ReadyListener {
    pub(super) fn new(listener: &TcpListener, cancellation: Arc<Cancellation>) -> io::Result<Self> {
        let poll = mio::Poll::new()?;
        let mut listener = mio::net::TcpListener::from_std(listener.try_clone()?);
        poll.registry()
            .register(&mut listener, LISTENER, mio::Interest::READABLE)?;
        let wake = Arc::new(mio::Waker::new(poll.registry(), CANCEL)?);
        // register invokes the callback immediately if stop already won; a
        // later stop captures this same callback under the admission mutex.
        let registration = cancellation.register(WakePhase::SocketAndWriter, move || {
            let _ = wake.wake();
        });
        Ok(Self {
            _registration: registration,
            listener,
            poll,
            events: mio::Events::with_capacity(4),
            cancellation,
            #[cfg(test)]
            probe: None,
        })
    }

    pub(super) fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        // ALWAYS use registered Mio I/O. Its WouldBlock path rearms Windows
        // readiness; accepting through the original std clone would bypass it.
        self.listener
            .accept()
            .map(|(stream, address)| (TcpStream::from(stream), address))
    }

    pub(super) fn wait(&mut self) -> io::Result<Wake> {
        if self.cancellation.reason().is_some() {
            return Ok(Wake::Cancelled);
        }
        #[cfg(test)]
        let probe = self.probe.take();
        #[cfg(test)]
        if let Some(probe) = &probe {
            probe.entered.send(()).map_err(io::Error::other)?;
            probe
                .proceed
                .recv_timeout(Duration::from_secs(3))
                .map_err(io::Error::other)?;
        }
        match self
            .poll
            .poll(&mut self.events, Some(HOUSEKEEPING_INTERVAL))
        {
            Ok(()) => {}
            // Return to the owner for signal/idle checks, without extending the
            // housekeeping ceiling by retrying interrupted polls internally.
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                return Ok(Wake::Housekeeping);
            }
            Err(error) => return Err(error),
        }
        #[cfg(test)]
        if let Some(probe) = probe {
            probe
                .polled
                .send(self.events.iter().map(|event| event.token().0).collect())
                .map_err(io::Error::other)?;
        }
        if self.cancellation.reason().is_some() {
            return Ok(Wake::Cancelled);
        }
        if self.events.iter().any(|event| event.token() == LISTENER) {
            Ok(Wake::Readable)
        } else {
            // Timeout/spurious readiness never claims a connection. The owner
            // retries nonblocking accept then waits again on WouldBlock.
            Ok(Wake::Housekeeping)
        }
    }
}

#[cfg(test)]
struct PollProbe {
    entered: std::sync::mpsc::Sender<()>,
    proceed: std::sync::mpsc::Receiver<()>,
    polled: std::sync::mpsc::Sender<Vec<usize>>,
}

#[cfg(test)]
mod tests;
