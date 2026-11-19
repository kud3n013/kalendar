//! Background threads. The window never waits on the network or the
//! keyring: such work is handed to a thread started here. Each thread is
//! named, so it can be told apart in a debugger or a panic message, and a
//! thread that cannot be started is reported rather than taking the app down.

/// Runs `work` on a thread of its own, named `katlendar-<name>`. Nothing comes
/// back; for work whose result the window needs, see `App::in_background`.
pub fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(format!("katlendar-{name}")).spawn(work) {
        eprintln!("could not start the {name} thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn work_runs_on_a_named_thread() {
        let (tx, rx) = mpsc::channel();
        spawn("test", move || {
            let _ = tx.send(std::thread::current().name().map(str::to_string));
        });
        assert_eq!(rx.recv().unwrap().as_deref(), Some("katlendar-test"));
    }
}
