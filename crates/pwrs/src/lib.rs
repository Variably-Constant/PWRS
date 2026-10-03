//! Typed Rust surface over [`pwrs_sys`].
//!
//! * [`PsObject`] is an owned `GCHandle`: `Send`, freed on drop.
//! * [`Pipeline`] is a `!Send` token proving the holder is on the
//!   pipeline thread inside `Begin`/`Process`/`End`, the only place
//!   the engine permits stream writes.
//! * [`PsError`] carries an `ErrorCategory` and a terminating flag;
//!   the engine, not the cmdlet, applies `-ErrorAction`.
//! * [`export_module!`] emits the native exports a module needs.

pub use pwrs_macros::{cmdlet, completer, dynamic_params, on_import, on_remove, param, provider, psclass, psenum, psfield, psmethods, transform};
pub use pwrs_sys as sys;

pub mod class;
pub mod cmdlet;
pub mod completer;
pub mod convert;
pub mod cpu;
pub mod dynamic;
pub mod error;
pub mod events;
mod fallible;
pub mod helper;
pub mod host;
pub mod host_ui;
pub mod lifecycle;
pub mod object;
pub mod pinned;
pub mod pipeline;
pub mod progress;
pub mod provider;
pub mod proxy;
pub mod runtime;
pub mod surface;
pub mod task;
pub mod testing;
pub mod text;
pub mod thread;
pub mod transform;
pub mod trace;
pub mod types;
pub mod values;
pub mod worker;

#[cfg(test)]
mod convert_tests;

pub use class::PsTyped;
pub use cmdlet::{Cmdlet, CmdletBind, CmdletMeta};
pub use completer::{Completion, CompletionContext, CompletionKind, DynamicParam};
pub use convert::{FromPs, IntoPs, PsArray};
pub use dynamic::PsType;
pub use error::{ErrorCategory, PsError, PsResult};
pub use events::PsEvents;
pub use helper::helper_path;
pub use host::{attach_current_thread, AttachedThread};
pub use object::PsObject;
pub use pinned::{Pinned, PsMemory, PsMemoryView, PsRevocation};
pub use pipeline::{Order, Pipeline, StopSignal};
pub use task::PsTask;
pub use progress::Progress;
pub use provider::{Drive, Item, Provider};
pub use proxy::PsProxy;
pub use types::{PsBigInt, PsCredential, PsHashtable, PsReadOnlyTable, PsScriptBlock, PsSecureString};
pub use host_ui::HostUi;
pub use transform::TransformFn;
pub use values::{
    DateTimeKind, PsDateTime, PsDateTimeOffset, PsDecimal, PsDecimalBits, PsErrorRecord, PsGuid, PsTimeSpan,
};
pub use worker::{Worker, Workers};

pub mod prelude {
    pub use crate::{
        cmdlet, completer, dynamic_params, export_module, on_import, on_remove, param, provider, psclass, psenum, psfield, psmethods, transform, Cmdlet, CmdletMeta, Completion,
        CompletionContext, DateTimeKind, Drive, DynamicParam, ErrorCategory, FromPs, HostUi, IntoPs, Item, Order, Pipeline, Progress, Provider, PsArray,
        PsBigInt, PsCredential, PsDateTime, PsDateTimeOffset, PsDecimal, PsDecimalBits, PsError, PsErrorRecord, PsEvents, PsGuid, PsHashtable, PsMemory, PsMemoryView, PsObject,
        PsProxy, PsReadOnlyTable, PsResult, PsRevocation, PsScriptBlock, PsSecureString, PsTask, PsTimeSpan, PsType, PsTyped, StopSignal, TransformFn, Worker,
    };
}
