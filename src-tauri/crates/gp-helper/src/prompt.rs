//! One-shot question from the VPN worker thread to the GUI.
//!
//! The worker blocks in [`Prompt::wait`] while the stdin loop delivers the
//! answer with [`Prompt::answer`], or gives up with [`Prompt::cancel`].

use std::sync::{Condvar, Mutex};

enum State<T> {
	/// Nobody is waiting.
	Idle,
	Waiting,
	Answered(T),
	Cancelled,
}

pub struct Prompt<T> {
	state: Mutex<State<T>>,
	wake: Condvar,
}

impl<T> Default for Prompt<T> {
	fn default() -> Self {
		Self {
			state: Mutex::new(State::Idle),
			wake: Condvar::new(),
		}
	}
}

impl<T> Prompt<T> {
	/// Blocks until answered, returning `None` if cancelled.
	///
	/// A cancellation that arrives before the wait starts still applies, so a
	/// disconnect racing a prompt can never leave the worker blocked.
	pub fn wait(&self) -> Option<T> {
		let Ok(mut state) = self.state.lock() else {
			return None;
		};
		if matches!(*state, State::Cancelled) {
			return None;
		}
		*state = State::Waiting;
		loop {
			match std::mem::replace(&mut *state, State::Waiting) {
				State::Answered(value) => {
					*state = State::Idle;
					return Some(value);
				}
				// Left in place: once an attempt is cancelled, every later
				// prompt in it should fail fast rather than block again.
				State::Cancelled => return None,
				previous => {
					*state = previous;
					let Ok(next) = self.wake.wait(state) else {
						return None;
					};
					state = next;
				}
			}
		}
	}

	/// Delivers an answer. Ignored when nothing is waiting, so a stray reply
	/// cannot be picked up by the next question.
	pub fn answer(&self, value: T) {
		if let Ok(mut state) = self.state.lock() {
			if matches!(*state, State::Waiting) {
				*state = State::Answered(value);
				self.wake.notify_all();
			}
		}
	}

	pub fn cancel(&self) {
		if let Ok(mut state) = self.state.lock() {
			*state = State::Cancelled;
			self.wake.notify_all();
		}
	}

	/// Clears a cancellation so the prompt can be used by the next attempt.
	pub fn reset(&self) {
		if let Ok(mut state) = self.state.lock() {
			*state = State::Idle;
		}
	}
}

#[cfg(test)]
mod tests {
	use std::{sync::Arc, time::Duration};

	use super::*;

	#[test]
	fn an_answer_reaches_the_waiter() {
		let prompt = Arc::new(Prompt::<String>::default());
		let answering = prompt.clone();
		std::thread::spawn(move || {
			// Give the waiter time to register before answering.
			for _ in 0..100 {
				std::thread::sleep(Duration::from_millis(5));
				answering.answer("123456".into());
			}
		});
		assert_eq!(prompt.wait().as_deref(), Some("123456"));
	}

	#[test]
	fn cancelling_releases_the_waiter() {
		let prompt = Arc::new(Prompt::<bool>::default());
		let cancelling = prompt.clone();
		std::thread::spawn(move || {
			for _ in 0..100 {
				std::thread::sleep(Duration::from_millis(5));
				cancelling.cancel();
			}
		});
		assert_eq!(prompt.wait(), None);
	}

	#[test]
	fn an_answer_with_nobody_waiting_is_dropped() {
		let prompt = Prompt::<bool>::default();
		prompt.answer(true);
		assert!(matches!(*prompt.state.lock().unwrap(), State::Idle));
	}

	#[test]
	fn cancelling_before_the_wait_starts_still_applies() {
		let prompt = Prompt::<bool>::default();
		prompt.cancel();
		assert_eq!(prompt.wait(), None);
		// And it stays cancelled for the rest of the attempt.
		assert_eq!(prompt.wait(), None);
	}

	#[test]
	fn resetting_makes_the_prompt_usable_again() {
		let prompt = Arc::new(Prompt::<bool>::default());
		prompt.cancel();
		assert_eq!(prompt.wait(), None);
		prompt.reset();
		let answering = prompt.clone();
		std::thread::spawn(move || {
			for _ in 0..100 {
				std::thread::sleep(Duration::from_millis(5));
				answering.answer(true);
			}
		});
		assert_eq!(prompt.wait(), Some(true));
	}
}
