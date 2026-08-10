//! Wall-clock budget for one connection attempt.
//!
//! Nothing below the helper can be relied on to give up on its own: a portal
//! that accepts the TCP connection and then goes quiet, a gateway that never
//! finishes the tunnel handshake, or a vpnc-script that hangs would otherwise
//! leave the attempt running until the user notices.

use std::{
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	},
	thread,
	time::Duration,
};

/// How long an attempt may spend on work of its own before it is abandoned.
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Granularity of the countdown. Short enough that the clock restarts promptly
/// once the user answers a prompt, long enough to cost nothing while idle.
const TICK: Duration = Duration::from_millis(200);

/// Counts an attempt down and calls back once its budget is spent.
///
/// The clock only advances while `charging` reports that the attempt is working
/// on its own behalf: time the user spends deciding whether to trust a
/// certificate, typing a one-time code or picking a gateway is not time the
/// connection is stuck, and must not count against it.
pub struct Deadline {
	done: Arc<AtomicBool>,
}

impl Deadline {
	/// Starts the countdown. `expired` runs at most once, and never after
	/// [`Deadline::finish`] or a drop has stopped the clock.
	pub fn start<C, E>(limit: Duration, charging: C, expired: E) -> Self
	where
		C: Fn() -> bool + Send + 'static,
		E: FnOnce() + Send + 'static,
	{
		let done = Arc::new(AtomicBool::new(false));
		let watcher = done.clone();
		thread::spawn(move || {
			let mut spent = Duration::ZERO;
			while spent < limit {
				thread::sleep(TICK);
				if watcher.load(Ordering::SeqCst) {
					return;
				}
				if charging() {
					spent += TICK;
				}
			}
			// Claims the outcome, so an attempt that succeeded in the same
			// instant is never torn down by its own watchdog.
			if !watcher.swap(true, Ordering::SeqCst) {
				expired();
			}
		});
		Self { done }
	}

	/// Stops the countdown, for good.
	pub fn finish(&self) {
		self.done.store(true, Ordering::SeqCst);
	}
}

impl Drop for Deadline {
	fn drop(&mut self) {
		self.finish();
	}
}

#[cfg(test)]
mod tests {
	use std::sync::{
		atomic::{AtomicUsize, Ordering},
		Mutex,
	};

	use super::*;

	/// Blocks until `condition` holds, or gives up after `limit`.
	fn wait_for(limit: Duration, condition: impl Fn() -> bool) -> bool {
		let started = std::time::Instant::now();
		while started.elapsed() < limit {
			if condition() {
				return true;
			}
			thread::sleep(Duration::from_millis(10));
		}
		condition()
	}

	#[test]
	fn an_attempt_that_overruns_its_budget_expires() {
		let fired = Arc::new(AtomicBool::new(false));
		let flag = fired.clone();
		let _deadline = Deadline::start(
			Duration::from_millis(400),
			|| true,
			move || flag.store(true, Ordering::SeqCst),
		);
		assert!(wait_for(Duration::from_secs(5), || fired.load(Ordering::SeqCst)));
	}

	#[test]
	fn time_that_is_not_charged_never_expires_the_attempt() {
		let fired = Arc::new(AtomicBool::new(false));
		let flag = fired.clone();
		let _deadline = Deadline::start(
			Duration::from_millis(200),
			// Standing in for a prompt the user has not answered yet.
			|| false,
			move || flag.store(true, Ordering::SeqCst),
		);
		thread::sleep(Duration::from_millis(800));
		assert!(!fired.load(Ordering::SeqCst));
	}

	#[test]
	fn finishing_stops_the_countdown() {
		let fired = Arc::new(AtomicBool::new(false));
		let flag = fired.clone();
		let deadline = Deadline::start(
			Duration::from_millis(200),
			|| true,
			move || flag.store(true, Ordering::SeqCst),
		);
		deadline.finish();
		thread::sleep(Duration::from_millis(800));
		assert!(!fired.load(Ordering::SeqCst));
	}

	#[test]
	fn expiry_happens_once() {
		let count = Arc::new(AtomicUsize::new(0));
		let counter = count.clone();
		// Kept alive so the drop does not stop the clock before it fires.
		let _held: Mutex<Option<Deadline>> = Mutex::new(Some(Deadline::start(
			Duration::from_millis(200),
			|| true,
			move || {
				counter.fetch_add(1, Ordering::SeqCst);
			},
		)));
		thread::sleep(Duration::from_millis(1200));
		assert_eq!(count.load(Ordering::SeqCst), 1);
	}
}
