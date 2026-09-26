//! Counters for the per-call path, and the trace that prints them.
//!
//! Every counter is a relaxed atomic bumped on a path that already
//! crosses the interop boundary. The sites are skipped when
//! `PWRS_TRACE` is unset, so an untraced call pays a cached relaxed
//! load and a branch. The fake host in [`crate::testing`] keeps them
//! whatever the variable says: its tests read the counters to prove
//! which entry a value took.
//!
//! `PWRS_TRACE=1` prints a summary line to stderr every
//! [`SUMMARY_EVERY`] instance creations. `PWRS_TRACE=2` prints a line
//! per event, which is only usable for a handful of invocations.
//!
//! The variable is read with `getenv`, so it has to be in the
//! environment the process inherits; assigning it inside PowerShell
//! through `$env:` reaches the managed counters only.
//!
//! The instance counters answer the question the managed side cannot:
//! whether the engine disposed every cmdlet it made. `live` climbing
//! without bound is a leak of one [`crate::runtime`] instance per
//! invocation.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

/// Instances created by `pwrs_cmdlet_create`.
pub static INSTANCES_CREATED: AtomicU64 = AtomicU64::new(0);
/// Instances freed by `pwrs_cmdlet_release`.
pub static INSTANCES_RELEASED: AtomicU64 = AtomicU64::new(0);
/// Phase calls through `pwrs_cmdlet_invoke`.
pub static INVOCATIONS: AtomicU64 = AtomicU64::new(0);
/// Objects written through the direct scalar entries.
pub static DIRECT_WRITES: AtomicU64 = AtomicU64::new(0);
/// Objects written through the generic handle entry.
pub static HANDLE_WRITES: AtomicU64 = AtomicU64::new(0);
/// Parameter binds actually performed (a phase whose block was not
/// dirty skips its bind).
pub static BINDS: AtomicU64 = AtomicU64::new(0);
/// Phases observed running the trait's default body and recorded as
/// empty for their type; at most two per cmdlet type.
pub static PHASES_LEARNED: AtomicU64 = AtomicU64::new(0);
/// Nanoseconds spent in `bind`, summed. Counted only while tracing.
pub static BIND_NS: AtomicU64 = AtomicU64::new(0);
/// Nanoseconds spent in the cmdlet's own phase body, summed. Counted
/// only while tracing.
pub static BODY_NS: AtomicU64 = AtomicU64::new(0);

/// How many creations between summary lines at `PWRS_TRACE=1`.
pub const SUMMARY_EVERY: u64 = 10_000;

const UNREAD: u8 = u8::MAX;
static LEVEL: AtomicU8 = AtomicU8::new(UNREAD);

/// The trace level from `PWRS_TRACE`, read once and cached. 0 is off.
///
/// The environment lookup sits in a `#[cold]` callee, so a call site
/// inlines one relaxed byte load and a compare.
#[inline(always)]
pub fn level() -> u8 {
    let cached = LEVEL.load(Ordering::Relaxed);
    if cached != UNREAD {
        return cached;
    }
    resolve_level()
}

#[cold]
fn resolve_level() -> u8 {
    let parsed = match std::env::var("PWRS_TRACE") {
        Ok(v) => v.trim().parse::<u8>().unwrap_or(if v.trim().is_empty() { 0 } else { 1 }),
        Err(_unset) => 0,
    };
    LEVEL.store(parsed, Ordering::Relaxed);
    parsed
}

/// A snapshot of every counter, for a report or a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counters {
    pub created: u64,
    pub released: u64,
    pub invocations: u64,
    pub direct_writes: u64,
    pub handle_writes: u64,
    pub binds: u64,
    pub bind_ns: u64,
    pub body_ns: u64,
    pub phases_learned: u64,
}

impl Counters {
    /// Instances made but not yet freed.
    #[inline]
    pub fn live(&self) -> u64 {
        self.created.saturating_sub(self.released)
    }
}

/// Reads every counter. Not atomic as a group; for diagnosis, not control flow.
pub fn snapshot() -> Counters {
    Counters {
        created: INSTANCES_CREATED.load(Ordering::Relaxed),
        released: INSTANCES_RELEASED.load(Ordering::Relaxed),
        invocations: INVOCATIONS.load(Ordering::Relaxed),
        direct_writes: DIRECT_WRITES.load(Ordering::Relaxed),
        handle_writes: HANDLE_WRITES.load(Ordering::Relaxed),
        binds: BINDS.load(Ordering::Relaxed),
        bind_ns: BIND_NS.load(Ordering::Relaxed),
        body_ns: BODY_NS.load(Ordering::Relaxed),
        phases_learned: PHASES_LEARNED.load(Ordering::Relaxed),
    }
}

static LAST_BINDS: AtomicU64 = AtomicU64::new(0);
static LAST_BIND_NS: AtomicU64 = AtomicU64::new(0);
static LAST_INVOCATIONS: AtomicU64 = AtomicU64::new(0);
static LAST_BODY_NS: AtomicU64 = AtomicU64::new(0);

#[inline]
fn avg(ns: u64, n: u64) -> u64 {
    ns.checked_div(n).unwrap_or(0)
}

/// Writes the current counters to stderr with `what` naming the event.
/// The averages are per bind and per invocation over the window since
/// the previous report, so warm-up does not sit in the steady state.
pub fn report(what: &str) {
    let c = snapshot();
    let binds = c.binds - LAST_BINDS.swap(c.binds, Ordering::Relaxed);
    let bind_ns = c.bind_ns - LAST_BIND_NS.swap(c.bind_ns, Ordering::Relaxed);
    let invocations = c.invocations - LAST_INVOCATIONS.swap(c.invocations, Ordering::Relaxed);
    let body_ns = c.body_ns - LAST_BODY_NS.swap(c.body_ns, Ordering::Relaxed);
    eprintln!(
        "pwrs trace {what}: created={} released={} live={} invocations={} binds={} direct_writes={} handle_writes={} phases_learned={} window: bind_ns_avg={} body_ns_avg={}",
        c.created,
        c.released,
        c.live(),
        c.invocations,
        c.binds,
        c.direct_writes,
        c.handle_writes,
        c.phases_learned,
        avg(bind_ns, binds),
        avg(body_ns, invocations)
    );
}

#[inline]
pub(crate) fn on_phase_learned() {
    PHASES_LEARNED.fetch_add(1, Ordering::Relaxed);
    if level() >= 2 {
        report("phase learned");
    }
}

/// A timestamp for [`on_bind`] or [`on_body`]; `None` when tracing
/// is off, so the untraced path pays one atomic load and a branch.
#[inline]
pub(crate) fn start() -> Option<std::time::Instant> {
    if level() >= 1 {
        Some(std::time::Instant::now())
    } else {
        None
    }
}

static FORCED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Keeps the counters whatever `PWRS_TRACE` says. The fake host turns
/// this on, because a unit test reads the counters to prove which
/// path a value took.
pub(crate) fn force_counting() {
    FORCED.store(true, Ordering::Relaxed);
}

/// Whether the counters are being kept: two relaxed loads of cached
/// values.
#[inline(always)]
pub(crate) fn counting() -> bool {
    level() > 0 || FORCED.load(Ordering::Relaxed)
}

#[inline]
pub(crate) fn on_bind(started: Option<std::time::Instant>) {
    if !counting() {
        return;
    }
    BINDS.fetch_add(1, Ordering::Relaxed);
    if let Some(t) = started {
        BIND_NS.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}

#[inline]
pub(crate) fn on_body(started: Option<std::time::Instant>) {
    if let Some(t) = started {
        BODY_NS.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}

#[inline]
pub(crate) fn on_create() {
    if !counting() {
        return;
    }
    let n = INSTANCES_CREATED.fetch_add(1, Ordering::Relaxed) + 1;
    let lvl = level();
    if lvl >= 2 || (lvl == 1 && n.is_multiple_of(SUMMARY_EVERY)) {
        report("create");
    }
}

#[inline]
pub(crate) fn on_release() {
    if !counting() {
        return;
    }
    INSTANCES_RELEASED.fetch_add(1, Ordering::Relaxed);
    if level() >= 2 {
        report("release");
    }
}

#[inline]
pub(crate) fn on_invoke() {
    if !counting() {
        return;
    }
    INVOCATIONS.fetch_add(1, Ordering::Relaxed);
    if level() >= 2 {
        report("invoke");
    }
}

#[inline]
pub(crate) fn on_direct_write() {
    if counting() {
        DIRECT_WRITES.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline]
pub(crate) fn on_handle_write() {
    if counting() {
        HANDLE_WRITES.fetch_add(1, Ordering::Relaxed);
    }
}
