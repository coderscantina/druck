//! Shared by the examples. Layout reads a deeply nested theme, which a debug build does in several hundred
//! kilobytes of stack, and a Windows main thread has one megabyte. A client picks its own threads, so the examples
//! run their work on one with room.

/// Runs `work` on a thread with 16 MB of stack and returns its result.
pub fn on_big_stack<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn_scoped(scope, work)
            .expect("the system starts a thread")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// What an example returns: any failure, printed by `main`.
pub type Outcome = Result<(), Box<dyn std::error::Error + Send + Sync>>;
