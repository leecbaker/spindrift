//! Feature-gated call-path stack diagnostics for recursive layout on macOS.
//!
//! Every [`StackProfileScope`] records one active recursive boundary. At each
//! new stack high-water mark, the profiler retains the complete active path.
//! The scope and path bookkeeping are compiled only with the `stack-profile`
//! feature because the guard changes the measured frame shape.

use std::cell::RefCell;
use std::fmt;

const STACK_REPORT_INTERVAL_BYTES: usize = 64 * 1024;

std::thread_local! {
    static STACK_PROFILE: RefCell<StackProfile> = RefCell::new(StackProfile::default());
}

/// Enter a profiled recursive layout boundary.
///
/// The returned guard removes the boundary from the active path when dropped.
/// macOS reports the high address and size of a downward-growing pthread
/// stack, so the address of this function's local marker gives a conservative
/// high-water observation.
#[inline(never)]
pub(in crate::layout) fn enter(label: &'static str) -> StackProfileScope {
    let marker = 0_u8;
    let stack_pointer = std::hint::black_box(&marker) as *const u8 as usize;
    let depth = STACK_PROFILE.with(|profile| {
        let mut profile = profile.borrow_mut();
        if !profile.initialized {
            profile.initialize();
        }
        profile.active.push(ActiveScope {
            label,
            source_index: None,
        });
        profile.observe(stack_pointer);
        profile.active.len()
    });
    StackProfileScope { label, depth }
}

/// An active profiled recursive layout boundary.
#[must_use = "the scope guard must remain live for the recursive call path"]
pub(in crate::layout) struct StackProfileScope {
    label: &'static str,
    depth: usize,
}

impl StackProfileScope {
    /// Update this boundary's current source index before it recurses.
    pub(in crate::layout) fn set_source_index(&mut self, source_index: usize) {
        STACK_PROFILE.with(|profile| {
            let mut profile = profile.borrow_mut();
            let scope = profile
                .active
                .get_mut(self.depth - 1)
                .expect("stack-profile scope must remain active while indexed");
            debug_assert_eq!(scope.label, self.label);
            scope.source_index = Some(source_index);
        });
    }
}

impl Drop for StackProfileScope {
    fn drop(&mut self) {
        STACK_PROFILE.with(|profile| {
            let mut profile = profile.borrow_mut();
            let scope = profile
                .active
                .pop()
                .expect("stack-profile scopes must drop in LIFO order");
            debug_assert_eq!(scope.label, self.label);
            debug_assert_eq!(profile.active.len() + 1, self.depth);
        });
    }
}

#[derive(Clone, Copy)]
struct ActiveScope {
    label: &'static str,
    source_index: Option<usize>,
}

#[derive(Default)]
struct StackProfile {
    initialized: bool,
    stack_top: usize,
    stack_size_bytes: usize,
    high_water_bytes: usize,
    next_report_bytes: usize,
    active: Vec<ActiveScope>,
    high_water_path: Vec<ActiveScope>,
}

impl StackProfile {
    fn initialize(&mut self) {
        self.initialized = true;
        // `pthread_get_stackaddr_np` returns the high address of a downward-
        // growing pthread stack on Darwin.
        let thread = unsafe { pthread_self() };
        self.stack_top = unsafe { pthread_get_stackaddr_np(thread) as usize };
        self.stack_size_bytes = unsafe { pthread_get_stacksize_np(thread) };
        self.next_report_bytes = STACK_REPORT_INTERVAL_BYTES;
        log::info!(
            target: "spindrift::stack_profile",
            "stack_bytes={} report_interval_bytes={STACK_REPORT_INTERVAL_BYTES}",
            self.stack_size_bytes,
        );
    }

    fn observe(&mut self, stack_pointer: usize) {
        if stack_pointer > self.stack_top {
            return;
        }
        let used_bytes = self.stack_top - stack_pointer;
        if used_bytes <= self.high_water_bytes {
            return;
        }
        self.high_water_bytes = used_bytes;
        self.high_water_path.clear();
        self.high_water_path.extend_from_slice(&self.active);

        if used_bytes < self.next_report_bytes {
            return;
        }
        while self.next_report_bytes <= used_bytes {
            self.next_report_bytes += STACK_REPORT_INTERVAL_BYTES;
        }
        log::info!(
            target: "spindrift::stack_profile",
            "used_bytes={used_bytes} stack_bytes={} percent={:.1} path={}",
            self.stack_size_bytes,
            used_bytes as f64 * 100.0 / self.stack_size_bytes as f64,
            StackPath(&self.high_water_path),
        );
    }
}

struct StackPath<'a>(&'a [ActiveScope]);

impl fmt::Display for StackPath<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, scope) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(" > ")?;
            }
            formatter.write_str(scope.label)?;
            if let Some(source_index) = scope.source_index {
                write!(formatter, "[source_index={source_index}]")?;
            }
        }
        Ok(())
    }
}

unsafe extern "C" {
    fn pthread_self() -> *mut std::ffi::c_void;
    fn pthread_get_stackaddr_np(thread: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn pthread_get_stacksize_np(thread: *mut std::ffi::c_void) -> usize;
}
