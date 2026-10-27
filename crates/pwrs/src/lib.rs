//! Typed Rust surface over [`pwrs_sys`].
//!
//! * [`PsObject`] is a `GCHandle`: `Send`, freed on drop.
//! * [`Pipeline`] is a `!Send` token that proves its code runs on the
//!   pipeline thread inside `Begin`/`Process`/`End`, the only place
//!   the engine permits stream writes.
//! * [`PsError`] has an `ErrorCategory` and a terminating flag;
//!   the engine, not the cmdlet, applies `-ErrorAction`.
//! * [`export_module!`] emits the native exports a module needs.

pub use pwrs_macros::{cmdlet, completer, dynamic_params, on_import, on_load, on_remove, on_unload, param, provider, psclass, psenum, psfield, psmethods, transform};
pub use pwrs_sys as sys;

pub mod class;
pub mod color;
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
pub mod stop;
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
pub use color::ConsoleColor;
pub use cmdlet::{Cmdlet, CmdletBind, CmdletMeta};
pub use completer::{Completion, CompletionContext, CompletionKind, DynamicParam};
pub use convert::{FromPs, IntoPs, PsArray};
pub use dynamic::PsType;
pub use error::{ErrorCategory, PsError, PsResult};
pub use events::PsEvents;
pub use helper::{helper_path, native_file_path};
pub use host::{attach_current_thread, AttachedThread};
pub use object::PsObject;
pub use pinned::{Pinned, PsArrayMut, PsMemory, PsMemoryView, PsRevocation};
pub use pipeline::{Order, Pipeline, StopSignal};
pub use stop::{StopWaker, StopWakerSlot};
pub use task::PsTask;
pub use progress::Progress;
pub use provider::{Call, Drive, Item, Provider, SecurityDescriptor, SecurityKind, SecuritySections};
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
        cmdlet, completer, dynamic_params, export_module, on_import, on_load, on_remove, on_unload, param, provider, psclass, psenum, psfield, psmethods, transform, Call, Cmdlet, CmdletMeta,
        Completion, ConsoleColor,
        CompletionContext, DateTimeKind, Drive, DynamicParam, ErrorCategory, FromPs, HostUi, IntoPs, Item, Order, Pipeline, Progress, Provider, PsArray, PsArrayMut,
        PsBigInt, PsCredential, PsDateTime, PsDateTimeOffset, PsDecimal, PsDecimalBits, PsError, PsErrorRecord, PsEvents, PsGuid, PsHashtable, PsMemory, PsMemoryView, PsObject,
        PsProxy, PsReadOnlyTable, PsResult, PsRevocation, PsScriptBlock, PsSecureString, PsTask, PsTimeSpan, PsType, PsTyped, SecurityDescriptor, SecurityKind, SecuritySections,
        StopSignal, StopWaker, TransformFn, Worker,
    };
}
