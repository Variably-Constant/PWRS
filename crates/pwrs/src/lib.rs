//! Typed Rust surface over [`pwrs_sys`].
//!
//! * [`PsObject`] is a `GCHandle`: `Send`, freed on drop.
//! * [`Pipeline`] is a `!Send` token that proves its code runs on the
//!   pipeline thread inside `Begin`/`Process`/`End`, the only place
//!   the engine permits stream writes.
//! * [`PsError`] has an `ErrorCategory` and a terminating flag;
//!   the engine, not the cmdlet, applies `-ErrorAction`.
//! * [`export_module!`] emits the native exports a module needs.

#![cfg_attr(all(target_arch = "arm", feature = "arm-neon"), feature(stdarch_arm_neon_intrinsics, arm_target_feature, stdarch_aarch32_crc32))]

pub use pwrs_macros::{cmdlet, completer, dynamic_params, on_import, on_load, on_remove, on_unload, param, provider, psclass, psenum, psfield, psmethods, tiered, transform};
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
pub mod job;
pub mod lifecycle;
pub mod object;
pub mod pinned;
pub mod pipeline;
pub mod progress;
pub mod provider;
pub mod proxy;
pub mod runtime;
pub mod simd;
pub mod steppable;
pub mod stop;
mod streams;
pub mod surface;
pub mod task;
pub mod testing;
pub mod text;
pub mod thread;
pub mod transaction;
pub mod transform;
pub mod trace;
pub mod types;
pub mod values;
pub mod worker;
pub mod wrapper;

#[cfg(test)]
mod convert_tests;

pub use class::PsTyped;
pub use color::ConsoleColor;
pub use cmdlet::{Cmdlet, CmdletBind, CmdletMeta};
pub use completer::{Completion, CompletionContext, CompletionKind, DynamicParam};
pub use convert::{FromPs, IntoPs, PsArray};
pub use dynamic::PsType;
pub use error::{ErrorCategory, PsError, PsResult};
pub use events::{PsEventRecord, PsEventSubscription, PsEvents, PsSubscription};
pub use helper::{helper_path, library_folder, native_file_path};
pub use host::{attach_current_thread, AttachedThread};
pub use job::{CmdletJob, JobBody, JobContext};
pub use streams::Streams;
pub use object::PsObject;
pub use pinned::{Pinned, PsArrayMut, PsMemory, PsMemoryView, PsRevocation, PsStringUnits};
pub use pipeline::{Order, Pipeline, PsPaging, ShouldProcessReason, StopSignal};
pub use steppable::PsSteppable;
pub use stop::{StopWaker, StopWakerSlot};
pub use task::PsTask;
pub use progress::Progress;
pub use provider::{Call, Drive, Item, Provider, SecurityDescriptor, SecurityKind, SecuritySections};
pub use proxy::{PsProxy, PsThis};
pub use types::{PsBigInt, PsCredential, PsHashtable, PsReadOnlyTable, PsScriptBlock, PsSecureString, PsString};
pub use host_ui::HostUi;
pub use lifecycle::{Removal, RemovalCause};
pub use transaction::{Enlistment, PsTransaction};
pub use transform::TransformFn;
pub use values::{
    DateTimeKind, PsDateTime, PsDateTimeOffset, PsDecimal, PsDecimalBits, PsErrorRecord, PsGuid, PsTimeSpan,
};
pub use worker::{Worker, Workers};
pub use wrapper::Wrapper;

pub mod prelude {
    pub use crate::{
        cmdlet, completer, dynamic_params, export_module, on_import, on_load, on_remove, on_unload, param, provider, psclass, psenum, psfield, psmethods, transform, Call, Cmdlet, CmdletJob,
        CmdletMeta, Completion, ConsoleColor,
        CompletionContext, DateTimeKind, Drive, DynamicParam, Enlistment, ErrorCategory, FromPs, HostUi, IntoPs, Item, JobBody, JobContext, Order, Pipeline, Progress, Provider, PsArray,
        PsArrayMut, PsBigInt, PsCredential, PsDateTime, PsDateTimeOffset, PsDecimal, PsDecimalBits, PsError, PsErrorRecord, PsEventRecord, PsEventSubscription, PsEvents, PsGuid,
        PsHashtable, PsMemory, PsMemoryView, PsObject, PsPaging, PsProxy, PsReadOnlyTable, PsResult, PsRevocation, PsScriptBlock, PsSecureString, PsSteppable, PsString, PsStringUnits,
        PsSubscription, PsTask, PsThis,
        PsTimeSpan, PsTransaction, PsType, PsTyped, Removal, RemovalCause, SecurityDescriptor, SecurityKind, SecuritySections, ShouldProcessReason, StopSignal, StopWaker, Streams,
        TransformFn, Worker, Wrapper,
    };
}
