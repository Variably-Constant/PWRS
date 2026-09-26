//! Native export implementations and the tables behind them.
//! `export_module!` emits the `#[no_mangle]` exports that forward
//! here so every module shares one implementation.

use crate::class::ClassEntry;
use crate::cmdlet::{CmdletEntry, InstanceHeader};
use crate::completer::{CompleterEntry, CompletionContext, DynamicParam};
use crate::host::vtable;
use crate::lifecycle::{Lifecycle, OP_IMPORT, OP_REMOVE};
use crate::provider::ProviderEntry;
use crate::transform::TransformEntry;
use crate::{ErrorCategory, IntoPs, Pipeline, PsError, PsHashtable, PsObject, PsResult};
use core::ffi::c_void;
use pwrs_sys::{
    HostVTable, ModuleDescriptor, PsHandle, PsPhase, PsPhaseMask, PsStatus, PsStr16, PS_ERR_ABI_MISMATCH, PS_ERR_MANAGED_EXCEPTION,
    PS_ERR_NATIVE_PANIC, PS_OK,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::OnceLock;

static REGISTRY: OnceLock<&'static [CmdletEntry]> = OnceLock::new();
static CLASSES: OnceLock<&'static [ClassEntry]> = OnceLock::new();
static COMPLETERS: OnceLock<&'static [CompleterEntry]> = OnceLock::new();
static TRANSFORMS: OnceLock<&'static [TransformEntry]> = OnceLock::new();
pub type DynParamsFn = fn(&PsHashtable) -> PsResult<Vec<DynamicParam>>;
/// Keyed by cmdlet `Verb-Noun`, resolved to a cmdlet id at dispatch.
static DYNPARAMS: OnceLock<&'static [(&'static str, DynParamsFn)]> = OnceLock::new();
static PROVIDERS: OnceLock<&'static [ProviderEntry]> = OnceLock::new();
static LIFECYCLE: OnceLock<&'static Lifecycle> = OnceLock::new();
static DESCRIPTOR: OnceLock<String> = OnceLock::new();

/// Every table a module registers, passed as one value so that
/// adding a kind does not change the shape of the two calls that
/// take them. `export_module!` builds it from its own statics.
pub struct ModuleTables {
    pub cmdlets: &'static [CmdletEntry],
    pub classes: &'static [ClassEntry],
    pub completers: &'static [CompleterEntry],
    pub transforms: &'static [TransformEntry],
    /// Keyed by cmdlet `Verb-Noun`, resolved to a cmdlet id at dispatch.
    pub dynparams: &'static [(&'static str, DynParamsFn)],
    pub providers: &'static [ProviderEntry],
    pub lifecycle: &'static Lifecycle,
}

/// Installs the host table and records the module's cmdlet and class
/// tables. A second call with different tables is an ABI mismatch.
/// Panic output is silenced here because every panic is reported to
/// the engine as a terminating error instead.
///
/// # Safety
/// `table` must outlive the module.
pub unsafe fn module_init(table: *const HostVTable, tables: &ModuleTables) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let s = crate::host::install(table);
    if s != PS_OK {
        return s;
    }
    std::panic::set_hook(Box::new(|_info| {}));
    let stored = REGISTRY.get_or_init(|| tables.cmdlets);
    let stored_classes = CLASSES.get_or_init(|| tables.classes);
    COMPLETERS.get_or_init(|| tables.completers);
    TRANSFORMS.get_or_init(|| tables.transforms);
    DYNPARAMS.get_or_init(|| tables.dynparams);
    PROVIDERS.get_or_init(|| tables.providers);
    LIFECYCLE.get_or_init(|| tables.lifecycle);
    if stored.as_ptr() == tables.cmdlets.as_ptr() && stored_classes.as_ptr() == tables.classes.as_ptr() {
        PS_OK
    } else {
        PS_ERR_ABI_MISMATCH
    }
}

fn with_id(i: usize, descriptor: &str) -> String {
    let body = match descriptor.strip_prefix('{') {
        Some(rest) => rest,
        None => descriptor,
    };
    format!("{{\"id\":{i},{body}")
}

/// Compiles only for a `Send` type. `#[psclass]` evaluates it for a
/// proxy class, whose value is read, called and dropped on whatever
/// thread holds the managed object.
#[doc(hidden)]
pub const fn assert_send<T: Send>() {}

/// `s` escaped for the inside of a JSON string literal, without the
/// quotes. Used by generated descriptor functions.
#[doc(hidden)]
pub fn json_body(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// JSON descriptor: `{"abi":1,"name":...,"cmdlets":[...],"classes":[...]}`
/// where each entry is its `DESCRIPTOR` with the id inserted.
pub fn module_descriptor(module_name: &str, tables: &ModuleTables) -> ModuleDescriptor {
    let ModuleTables { cmdlets, classes, completers, transforms, dynparams, providers, lifecycle } = tables;
    let json = DESCRIPTOR.get_or_init(|| {
        let mut s = String::from("{\"abi\":1,\"name\":\"");
        s.push_str(module_name);
        s.push_str(&format!(
            "\",\"on_import\":{},\"on_remove\":{},\"cmdlets\":[",
            lifecycle.on_import.is_some(),
            lifecycle.on_remove.is_some()
        ));
        for (i, c) in cmdlets.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let has_dyn = dynparams.iter().any(|(n, _)| *n == c.name);
            let descriptor = (c.descriptor)();
            s.push_str(&format!("{{\"id\":{i},\"dynamic_params\":{has_dyn},{}", &descriptor[1..]));
        }
        s.push_str("],\"classes\":[");
        for (i, c) in classes.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&with_id(i, &(c.descriptor)()));
        }
        s.push_str("],\"completers\":[");
        for (i, c) in completers.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let (cmdlet, parameter) = match c.target.split_once('/') {
                Some((a, b)) => (a, b),
                None => (c.target, ""),
            };
            s.push_str(&format!("{{\"id\":{i},\"cmdlet\":\"{cmdlet}\",\"parameter\":\"{parameter}\"}}"));
        }
        s.push_str("],\"transforms\":[");
        for (i, t) in transforms.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let (cmdlet, parameter) = match t.target.split_once('/') {
                Some((a, b)) => (a, b),
                None => (t.target, ""),
            };
            s.push_str(&format!("{{\"id\":{i},\"cmdlet\":\"{cmdlet}\",\"parameter\":\"{parameter}\"}}"));
        }
        s.push_str("],\"providers\":[");
        for (i, p) in providers.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&with_id(i, p.descriptor));
        }
        s.push_str("]}");
        s
    });
    ModuleDescriptor { json_utf8: json.as_ptr(), json_len: json.len() }
}

/// Runs completer `completer_id` on the completion thread.
///
/// # Safety
/// String pointers are valid for the call; `out` and `err` are out-pointers.
pub unsafe fn completer_invoke(completer_id: u32, word: PsStr16, command: PsStr16, fake_bound: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let result = catch_unwind(AssertUnwindSafe(|| -> PsResult<PsObject> {
        let completers = COMPLETERS.get().ok_or_else(|| PsError::new(ErrorCategory::InvalidOperation, "PwrsNotInitialized", "module not initialized"))?;
        let entry = completers
            .get(completer_id as usize)
            .ok_or_else(|| PsError::new(ErrorCategory::InvalidArgument, "PwrsUnknownCompleter", format!("no completer with id {completer_id}")))?;
        let read = |s: PsStr16| -> PsResult<String> { crate::text::try_from_str16(s.ptr, s.len) };
        let bound = if fake_bound.is_null() {
            PsHashtable(PsObject::null())
        } else {
            PsHashtable(PsObject::from_raw((vtable().clone_handle)(fake_bound)))
        };
        let ctx = CompletionContext { word: read(word)?, command: read(command)?, bound };
        let completions = (entry.run)(&ctx)?;
        let objs: PsResult<Vec<PsObject>> = completions.iter().map(|c| c.to_object()).collect();
        objs?.into_ps()
    }));
    finish_object(result, out, err)
}

/// Runs transform `transform_id` over the value the binder is about
/// to assign, and returns what to assign instead.
///
/// Called during binding, before the cmdlet instance exists, so
/// nothing here touches an instance or a stream. A null value is
/// `$null` and reaches the transform as a null object.
///
/// # Safety
/// `out` and `err` are out-pointers.
pub unsafe fn transform_invoke(transform_id: u32, value: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let result = catch_unwind(AssertUnwindSafe(|| -> PsResult<PsObject> {
        let transforms = TRANSFORMS.get().ok_or_else(|| PsError::new(ErrorCategory::InvalidOperation, "PwrsNotInitialized", "module not initialized"))?;
        let entry = transforms
            .get(transform_id as usize)
            .ok_or_else(|| PsError::new(ErrorCategory::InvalidArgument, "PwrsUnknownTransform", format!("no transform with id {transform_id}")))?;
        let given = if value.is_null() { PsObject::null() } else { PsObject::from_raw((vtable().clone_handle)(value)) };
        (entry.run)(&given)
    }));
    finish_object(result, out, err)
}

/// Computes a cmdlet's dynamic parameters before binding.
///
/// # Safety
/// `out` and `err` are out-pointers.
pub unsafe fn dynparams_invoke(cmdlet_id: u32, cmdlet: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let result = catch_unwind(AssertUnwindSafe(|| -> PsResult<PsObject> {
        let table = DYNPARAMS.get().ok_or_else(|| PsError::new(ErrorCategory::InvalidOperation, "PwrsNotInitialized", "module not initialized"))?;
        let registry = REGISTRY.get().ok_or_else(|| PsError::new(ErrorCategory::InvalidOperation, "PwrsNotInitialized", "module not initialized"))?;
        let name = match registry.get(cmdlet_id as usize) {
            Some(entry) => entry.name,
            None => return PsObject::null().into_ps(),
        };
        let entry = table.iter().find(|(n, _)| *n == name);
        let run = match entry {
            Some((_, run)) => run,
            None => return PsObject::null().into_ps(),
        };
        let bound = PsHashtable(PsObject::from_raw((vtable().clone_handle)(cmdlet)));
        let params = run(&bound)?;
        crate::completer::pack_dynamic_params(&params)
    }));
    finish_object(result, out, err)
}

/// Runs the module's import or removal hook, and nothing when the
/// module declared none for that op.
///
/// # Safety
/// `err` is an out-pointer or null.
pub unsafe fn lifecycle_invoke(op: u32, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let result = catch_unwind(AssertUnwindSafe(|| -> PsResult<()> {
        let hooks = LIFECYCLE.get().ok_or_else(|| PsError::new(ErrorCategory::InvalidOperation, "PwrsNotInitialized", "module not initialized"))?;
        let hook = match op {
            OP_IMPORT => hooks.on_import,
            OP_REMOVE => hooks.on_remove,
            other => return Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsUnknownLifecycleOp", format!("no lifecycle op {other}"))),
        };
        match hook {
            Some(run) => run(),
            None => Ok(()),
        }
    }));
    match result {
        Ok(Ok(())) => PS_OK,
        Ok(Err(e)) => {
            if !err.is_null() {
                *err = string_handle(&e.to_string());
            }
            PS_ERR_MANAGED_EXCEPTION
        }
        Err(payload) => {
            if !err.is_null() {
                *err = string_handle(&panic_message(payload));
            }
            PS_ERR_NATIVE_PANIC
        }
    }
}

unsafe fn finish_object(result: std::thread::Result<PsResult<PsObject>>, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    match result {
        Ok(Ok(obj)) => {
            *out = obj.into_raw();
            PS_OK
        }
        Ok(Err(e)) => {
            if !err.is_null() {
                *err = string_handle(&e.to_string());
            }
            PS_ERR_MANAGED_EXCEPTION
        }
        Err(payload) => {
            if !err.is_null() {
                *err = string_handle(&panic_message(payload));
            }
            PS_ERR_NATIVE_PANIC
        }
    }
}

/// Runs one provider operation against a drive's instance (null for
/// the operations that need none). `args` is an `object[]` handle;
/// `out` receives an `object[]` handle of result rows.
///
/// # Safety
/// `out` and `err` are out-pointers; `args` is a live handle or null;
/// `instance` is null or a pointer a drive row of this provider
/// carried and that no `REMOVE_DRIVE` or `DROP_DRIVE` has freed.
pub unsafe fn provider_invoke(provider_id: u32, op_code: u32, instance: *mut c_void, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let result = catch_unwind(AssertUnwindSafe(|| -> PsResult<PsObject> {
        let providers = PROVIDERS.get().ok_or_else(|| PsError::new(ErrorCategory::InvalidOperation, "PwrsNotInitialized", "module not initialized"))?;
        let entry = providers
            .get(provider_id as usize)
            .ok_or_else(|| PsError::new(ErrorCategory::InvalidArgument, "PwrsUnknownProvider", format!("no provider with id {provider_id}")))?;
        let arg_vec: Vec<PsObject> = if args.is_null() {
            Vec::new()
        } else {
            let owned = PsObject::from_raw((vtable().clone_handle)(args));
            <Vec<PsObject> as crate::FromPs>::from_ps(&owned)?
        };
        let rows = (entry.dispatch)(instance, op_code, &arg_vec)?;
        rows.into_ps()
    }));
    finish_object(result, out, err)
}

/// The id `export_module!` assigned to a class by name.
pub fn class_id(name: &str) -> PsResult<u32> {
    let classes = CLASSES.get().ok_or_else(|| PsError::new(ErrorCategory::InvalidOperation, "PwrsNotInitialized", "module not initialized"))?;
    for (i, c) in classes.iter().enumerate() {
        if c.name == name {
            return Ok(i as u32);
        }
    }
    Err(PsError::new(ErrorCategory::InvalidOperation, "PwrsUnknownClass", format!("class {name} is not listed in export_module!")))
}

/// Instantiates a generated class through the module's factory.
///
/// # Safety
/// `block` must point to the class's field block (copied mode) or be
/// the boxed instance pointer (proxy mode).
pub unsafe fn factory_new(class_id: u32, block: *const c_void) -> PsResult<PsObject> {
    let mut out = PsHandle::NULL;
    let mut err = PsHandle::NULL;
    let status = (vtable().factory_new)(class_id, block, &mut out, &mut err);
    crate::pipeline::check(status, err)?;
    Ok(PsObject::from_raw(out))
}

fn panic_message(payload: Box<dyn core::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with non-string payload".to_string()
    }
}

unsafe fn string_handle(s: &str) -> PsHandle {
    let u: Vec<u16> = crate::text::to_utf16(s);
    (vtable().string_new)(PsStr16 { ptr: u.as_ptr(), len: u.len() })
}

/// Runs one lifecycle phase. Errors returned by the body are written
/// through the vtable (non-terminating immediately, terminating as a
/// pending record the managed side throws after this returns), so the
/// status is `PS_OK` unless the runtime itself failed or the body
/// panicked. On a panic `err` receives a managed string with the
/// message and the instance is discarded.
///
/// # Safety
/// `cmdlet` is the managed cmdlet's handle; `params` is its block.
pub unsafe fn cmdlet_invoke(instance: *mut c_void, phase: PsPhase, cmdlet: PsHandle, params: *const c_void, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    if instance.is_null() {
        return PS_ERR_ABI_MISMATCH;
    }
    crate::trace::on_invoke();
    let hdr = instance as *mut InstanceHeader;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let body = ((*hdr).run)(hdr, phase, cmdlet, params);
        match body {
            Ok(()) => Ok(()),
            Err(e) => {
                let ps = Pipeline::new(cmdlet, &(*hdr).stopping, &(*hdr).scratch);
                ps.write_error(&e).map_err(PhaseFailure)
            }
        }
    }));
    match result {
        Ok(Ok(())) => PS_OK,
        Ok(Err(failure)) => failure.report(err),
        Err(payload) => {
            let msg = panic_message(payload);
            if !err.is_null() {
                *err = string_handle(&msg);
            }
            PS_ERR_NATIVE_PANIC
        }
    }
}

/// Creates the Rust instance the managed cmdlet owns for its lifetime.
/// `phases` receives the type's learned phase mask.
///
/// # Safety
/// `out` receives a pointer freed exactly once by [`cmdlet_release`];
/// `phases` is an out-pointer or null.
pub unsafe fn cmdlet_create(cmdlet_id: u32, out: *mut *mut c_void, phases: *mut PsPhaseMask, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let registry = match REGISTRY.get() {
        Some(r) => *r,
        None => return PS_ERR_ABI_MISMATCH,
    };
    let entry = match registry.get(cmdlet_id as usize) {
        Some(e) => e,
        None => {
            if !err.is_null() {
                *err = string_handle(&format!("no cmdlet with id {cmdlet_id} in this module"));
            }
            return PS_ERR_ABI_MISMATCH;
        }
    };
    let made = catch_unwind(AssertUnwindSafe(|| (entry.new)()));
    match made {
        Ok(hdr) => {
            *out = hdr as *mut c_void;
            if !phases.is_null() {
                *phases = (entry.phase_mask)().load(core::sync::atomic::Ordering::Relaxed);
            }
            crate::trace::on_create();
            PS_OK
        }
        Err(payload) => {
            if !err.is_null() {
                *err = string_handle(&panic_message(payload));
            }
            PS_ERR_NATIVE_PANIC
        }
    }
}

/// A runtime-level failure of a phase, distinct from a body error:
/// the body failed and reporting that failure through the vtable
/// failed too. Carries the reporting error.
struct PhaseFailure(PsError);

impl PhaseFailure {
    unsafe fn report(self, err: *mut PsHandle) -> PsStatus {
        if !err.is_null() {
            *err = string_handle(&self.0.to_string());
        }
        PS_ERR_MANAGED_EXCEPTION
    }
}

/// Sets the stop flag. Called from the engine's own thread while the
/// pipeline thread runs, so the flag is atomic and nothing else is
/// touched. The managed side guarantees the instance outlives this.
///
/// # Safety
/// `instance` is live: the managed owner clears its pointer before
/// releasing, so a disposed cmdlet never reaches here.
pub unsafe fn cmdlet_stop(instance: *mut c_void) {
    if instance.is_null() {
        return;
    }
    (*(instance as *const InstanceHeader)).stop();
}

/// Drops the instance. Called once, from Dispose.
///
/// # Safety
/// `instance` came from [`cmdlet_create`] and is released once.
pub unsafe fn cmdlet_release(instance: *mut c_void) {
    let _called_in = crate::host::CalledIn::enter();
    if instance.is_null() {
        return;
    }
    let hdr = instance as *mut InstanceHeader;
    ((*hdr).drop)(hdr);
    crate::trace::on_release();
    if crate::surface::enabled() {
        crate::surface::dump();
    }
}

fn class_entry(class_id: u32) -> PsResult<&'static ClassEntry> {
    let classes: &'static [ClassEntry] =
        CLASSES.get().ok_or_else(|| PsError::new(ErrorCategory::InvalidOperation, "PwrsNotInitialized", "module not initialized"))?;
    classes
        .get(class_id as usize)
        .ok_or_else(|| PsError::new(ErrorCategory::InvalidArgument, "PwrsUnknownClass", format!("no class with id {class_id}")))
}

/// Reads one field of a proxy instance; `out` receives a handle the
/// managed side owns.
///
/// # Safety
/// `instance` came from the class's proxy factory and is not yet dropped.
pub unsafe fn proxy_get(class_id: u32, field_id: u32, instance: *mut c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let result = catch_unwind(AssertUnwindSafe(|| -> PsResult<PsObject> { (class_entry(class_id)?.proxy_get)(instance, field_id) }));
    finish_object(result, out, err)
}

/// Runs one `#[psmethods]` method of a proxy instance; `out` receives
/// the result as a handle the managed side owns, null for a method
/// returning `()`.
///
/// # Safety
/// `instance` came from the class's proxy factory and is not yet
/// dropped, or is null for a static method or the constructor, which
/// run against no value; `args` points at the method's packed
/// argument block.
pub unsafe fn proxy_call(class_id: u32, method_id: u32, instance: *mut c_void, args: *const c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let result = catch_unwind(AssertUnwindSafe(|| -> PsResult<PsObject> { (class_entry(class_id)?.proxy_call)(instance, method_id, args) }));
    finish_object(result, out, err)
}

/// Frees a proxy instance. An unknown class id leaks the instance,
/// and says so on stderr, rather than freeing memory of an unknown
/// type.
///
/// # Safety
/// `instance` came from the class's proxy factory; called once.
pub unsafe fn proxy_drop(class_id: u32, instance: *mut c_void) {
    let _called_in = crate::host::CalledIn::enter();
    if instance.is_null() {
        return;
    }
    // Nothing here can return a status: the managed finalizer calls
    // it with no one left to tell. Each way of not freeing the value
    // says so on stderr rather than leaking in silence.
    let Some(classes) = CLASSES.get() else {
        eprintln!("pwrs: a proxy value of class {class_id} was not freed; the module was never initialized");
        return;
    };
    let Some(class) = classes.get(class_id as usize) else {
        eprintln!("pwrs: a proxy value was not freed; no class has id {class_id}");
        return;
    };
    let dropped = catch_unwind(AssertUnwindSafe(|| (class.proxy_drop)(instance)));
    match dropped {
        Ok(()) => {}
        Err(payload) => eprintln!("pwrs: {} panicked while being dropped: {}", class.name, panic_message(payload)),
    }
}

/// The bytes of native memory a proxy instance reports keeping alive,
/// which the runtime passes on to the garbage collector. The answer has
/// no error channel, so a class id with no class, or a `native_bytes`
/// function that panics, answers 0 and says so on stderr.
///
/// # Safety
/// `instance` came from the class's proxy factory and is not yet dropped.
pub unsafe fn proxy_bytes(class_id: u32, instance: *mut c_void) -> u64 {
    let _called_in = crate::host::CalledIn::enter();
    if instance.is_null() {
        return 0;
    }
    let class = match class_entry(class_id) {
        Ok(class) => class,
        Err(e) => {
            eprintln!("pwrs: the native bytes of a proxy value of class {class_id} were not read: {}", e.message);
            return 0;
        }
    };
    match catch_unwind(AssertUnwindSafe(|| (class.proxy_bytes)(instance))) {
        Ok(bytes) => bytes,
        Err(payload) => {
            eprintln!("pwrs: {} panicked while reporting its native bytes: {}", class.name, panic_message(payload));
            0
        }
    }
}

/// Emits the native exports for a module.
///
/// ```ignore
/// pwrs::export_module! {
///     name: "Hello",
///     cmdlets: [GetGreeting],
///     classes: [Person],
///     enums: [Color],
/// }
/// ```
///
/// Enums share the class table with classes; both get a class id
/// from their position in the combined list, classes first.
#[macro_export]
macro_rules! export_module {
    (name: $name:literal, cmdlets: [$($ty:ty),* $(,)?]
        $(, classes: [$($cls:ty),* $(,)?])?
        $(, enums: [$($enm:ty),* $(,)?])?
        $(, completers: [$($cmp:ty),* $(,)?])?
        $(, transforms: [$($tfm:ty),* $(,)?])?
        $(, dynamic_params: [$($dpc:ty),* $(,)?])?
        $(, providers: [$($prov:ty),* $(,)?])?
        $(, on_import: $imp:ty)?
        $(, on_remove: $rem:ty)?
        $(,)?
    ) => {
        static __PWRS_CMDLETS: &[$crate::cmdlet::CmdletEntry] = &[
            $( $crate::cmdlet::CmdletEntry {
                name: <$ty as $crate::cmdlet::CmdletMeta>::NAME,
                descriptor: <$ty as $crate::cmdlet::CmdletMeta>::descriptor,
                new: $crate::cmdlet::Instance::<$ty>::leak_new,
                phase_mask: <$ty as $crate::cmdlet::CmdletMeta>::phase_mask,
            } ),*
        ];

        static __PWRS_CLASSES: &[$crate::class::ClassEntry] = &[
            $( $( $crate::class::ClassEntry {
                name: <$cls as $crate::class::PsClassMeta>::NAME,
                descriptor: <$cls as $crate::class::PsClassMeta>::descriptor,
                proxy_get: <$cls as $crate::class::PsClassMeta>::proxy_get,
                proxy_call: <$cls as $crate::class::PsClassMeta>::proxy_call,
                proxy_drop: <$cls as $crate::class::PsClassMeta>::proxy_drop,
                proxy_bytes: <$cls as $crate::class::PsClassMeta>::proxy_bytes,
            }, )* )?
            $( $( $crate::class::ClassEntry {
                name: <$enm as $crate::class::PsClassMeta>::NAME,
                descriptor: <$enm as $crate::class::PsClassMeta>::descriptor,
                proxy_get: <$enm as $crate::class::PsClassMeta>::proxy_get,
                proxy_call: <$enm as $crate::class::PsClassMeta>::proxy_call,
                proxy_drop: <$enm as $crate::class::PsClassMeta>::proxy_drop,
                proxy_bytes: <$enm as $crate::class::PsClassMeta>::proxy_bytes,
            }, )* )?
        ];

        static __PWRS_COMPLETERS: &[$crate::completer::CompleterEntry] = &[
            $( $( $crate::completer::CompleterEntry {
                target: <$cmp as $crate::completer::CompleterFn>::TARGET,
                run: <$cmp as $crate::completer::CompleterFn>::complete,
            } ),* )?
        ];

        static __PWRS_TRANSFORMS: &[$crate::transform::TransformEntry] = &[
            $( $( $crate::transform::TransformEntry {
                target: <$tfm as $crate::transform::TransformFn>::TARGET,
                run: <$tfm as $crate::transform::TransformFn>::transform,
            } ),* )?
        ];

        static __PWRS_DYNPARAMS: &[(&'static str, fn(&$crate::PsHashtable) -> $crate::PsResult<::std::vec::Vec<$crate::completer::DynamicParam>>)] = &[
            $( $( (
                <$dpc as $crate::cmdlet::CmdletMeta>::NAME,
                <$dpc as $crate::completer::DynamicParams>::dynamic_parameters,
            ) ),* )?
        ];

        static __PWRS_PROVIDERS: &[$crate::provider::ProviderEntry] = &[
            $( $( $crate::provider::ProviderEntry {
                name: <$prov as $crate::provider::ProviderMeta>::NAME,
                descriptor: <$prov as $crate::provider::ProviderMeta>::DESCRIPTOR,
                dispatch: $crate::provider::dispatch::<$prov>,
            } ),* )?
        ];

        static __PWRS_LIFECYCLE: $crate::lifecycle::Lifecycle = $crate::lifecycle::Lifecycle {
            on_import: [ $( Some(<$imp as $crate::lifecycle::OnImport>::run as fn() -> $crate::PsResult<()>), )? None ][0],
            on_remove: [ $( Some(<$rem as $crate::lifecycle::OnRemove>::run as fn() -> $crate::PsResult<()>), )? None ][0],
        };

        fn __pwrs_tables() -> $crate::runtime::ModuleTables {
            $crate::runtime::ModuleTables {
                cmdlets: __PWRS_CMDLETS,
                classes: __PWRS_CLASSES,
                completers: __PWRS_COMPLETERS,
                transforms: __PWRS_TRANSFORMS,
                dynparams: __PWRS_DYNPARAMS,
                providers: __PWRS_PROVIDERS,
                lifecycle: &__PWRS_LIFECYCLE,
            }
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_module_init(table: *const $crate::sys::HostVTable) -> $crate::sys::PsStatus {
            $crate::runtime::module_init(table, &__pwrs_tables())
        }

        #[no_mangle]
        pub extern "C" fn pwrs_module_descriptor() -> $crate::sys::ModuleDescriptor {
            $crate::runtime::module_descriptor($name, &__pwrs_tables())
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_module_lifecycle(op: u32, err: *mut $crate::sys::PsHandle) -> $crate::sys::PsStatus {
            $crate::runtime::lifecycle_invoke(op, err)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_provider_invoke(
            provider_id: u32,
            op_code: u32,
            instance: *mut ::core::ffi::c_void,
            args: $crate::sys::PsHandle,
            out: *mut $crate::sys::PsHandle,
            err: *mut $crate::sys::PsHandle,
        ) -> $crate::sys::PsStatus {
            $crate::runtime::provider_invoke(provider_id, op_code, instance, args, out, err)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_completer_invoke(
            completer_id: u32,
            word: $crate::sys::PsStr16,
            command: $crate::sys::PsStr16,
            fake_bound: $crate::sys::PsHandle,
            out: *mut $crate::sys::PsHandle,
            err: *mut $crate::sys::PsHandle,
        ) -> $crate::sys::PsStatus {
            $crate::runtime::completer_invoke(completer_id, word, command, fake_bound, out, err)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_transform_invoke(
            transform_id: u32,
            value: $crate::sys::PsHandle,
            out: *mut $crate::sys::PsHandle,
            err: *mut $crate::sys::PsHandle,
        ) -> $crate::sys::PsStatus {
            $crate::runtime::transform_invoke(transform_id, value, out, err)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_dynparams_invoke(
            cmdlet_id: u32,
            cmdlet: $crate::sys::PsHandle,
            out: *mut $crate::sys::PsHandle,
            err: *mut $crate::sys::PsHandle,
        ) -> $crate::sys::PsStatus {
            $crate::runtime::dynparams_invoke(cmdlet_id, cmdlet, out, err)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_cmdlet_create(
            cmdlet_id: u32,
            out: *mut *mut ::core::ffi::c_void,
            phases: *mut $crate::sys::PsPhaseMask,
            err: *mut $crate::sys::PsHandle,
        ) -> $crate::sys::PsStatus {
            $crate::runtime::cmdlet_create(cmdlet_id, out, phases, err)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_cmdlet_invoke(
            instance: *mut ::core::ffi::c_void,
            phase: $crate::sys::PsPhase,
            cmdlet: $crate::sys::PsHandle,
            params: *const ::core::ffi::c_void,
            err: *mut $crate::sys::PsHandle,
        ) -> $crate::sys::PsStatus {
            $crate::runtime::cmdlet_invoke(instance, phase, cmdlet, params, err)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_cmdlet_stop(instance: *mut ::core::ffi::c_void) {
            $crate::runtime::cmdlet_stop(instance)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_cmdlet_release(instance: *mut ::core::ffi::c_void) {
            $crate::runtime::cmdlet_release(instance)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_proxy_get(
            class_id: u32,
            field_id: u32,
            instance: *mut ::core::ffi::c_void,
            out: *mut $crate::sys::PsHandle,
            err: *mut $crate::sys::PsHandle,
        ) -> $crate::sys::PsStatus {
            $crate::runtime::proxy_get(class_id, field_id, instance, out, err)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_proxy_call(
            class_id: u32,
            method_id: u32,
            instance: *mut ::core::ffi::c_void,
            args: *const ::core::ffi::c_void,
            out: *mut $crate::sys::PsHandle,
            err: *mut $crate::sys::PsHandle,
        ) -> $crate::sys::PsStatus {
            $crate::runtime::proxy_call(class_id, method_id, instance, args, out, err)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_proxy_drop(class_id: u32, instance: *mut ::core::ffi::c_void) {
            $crate::runtime::proxy_drop(class_id, instance)
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwrs_proxy_bytes(class_id: u32, instance: *mut ::core::ffi::c_void) -> u64 {
            $crate::runtime::proxy_bytes(class_id, instance)
        }

        // The runtime checks these before any other export runs, so none
        // of them may execute code the compiler chose: the list is data,
        // and the two functions are naked, their bodies exactly the
        // instructions written. See `pwrs::cpu`.
        #[no_mangle]
        #[allow(non_upper_case_globals)]
        pub static pwrs_cpu_requirements: [u8; $crate::cpu::REQUIREMENTS_SIZE] = $crate::cpu::REQUIREMENTS;

        /// CPUID for `leaf` and `subleaf`; EAX, EBX, ECX and EDX to
        /// `out[0..4]`. RBX is callee-saved and CPUID writes it, so it
        /// waits in R10, which is volatile.
        #[cfg(all(target_arch = "x86_64", windows))]
        #[no_mangle]
        #[unsafe(naked)]
        pub unsafe extern "C" fn pwrs_cpuid(_leaf: u32, _subleaf: u32, _out: *mut u32) {
            ::core::arch::naked_asm!(
                "mov r10, rbx",
                "mov eax, ecx",
                "mov ecx, edx",
                "cpuid",
                "mov dword ptr [r8], eax",
                "mov dword ptr [r8 + 4], ebx",
                "mov dword ptr [r8 + 8], ecx",
                "mov dword ptr [r8 + 12], edx",
                "mov rbx, r10",
                "ret",
            )
        }

        /// The System V form: the arguments arrive in EDI, ESI and RDX,
        /// and CPUID writes EDX, so the output pointer moves to R8 first.
        #[cfg(all(target_arch = "x86_64", not(windows)))]
        #[no_mangle]
        #[unsafe(naked)]
        pub unsafe extern "C" fn pwrs_cpuid(_leaf: u32, _subleaf: u32, _out: *mut u32) {
            ::core::arch::naked_asm!(
                "mov r10, rbx",
                "mov r8, rdx",
                "mov eax, edi",
                "mov ecx, esi",
                "cpuid",
                "mov dword ptr [r8], eax",
                "mov dword ptr [r8 + 4], ebx",
                "mov dword ptr [r8 + 8], ecx",
                "mov dword ptr [r8 + 12], edx",
                "mov rbx, r10",
                "ret",
            )
        }

        /// XGETBV for register `xcr`, EDX:EAX joined into RAX. Faults
        /// unless CPUID leaf 1 reports OSXSAVE, which the caller checks.
        #[cfg(all(target_arch = "x86_64", windows))]
        #[no_mangle]
        #[unsafe(naked)]
        pub unsafe extern "C" fn pwrs_xgetbv(_xcr: u32) -> u64 {
            ::core::arch::naked_asm!("xgetbv", "shl rdx, 32", "or rax, rdx", "ret")
        }

        /// The System V form: the register number arrives in EDI.
        #[cfg(all(target_arch = "x86_64", not(windows)))]
        #[no_mangle]
        #[unsafe(naked)]
        pub unsafe extern "C" fn pwrs_xgetbv(_xcr: u32) -> u64 {
            ::core::arch::naked_asm!("mov ecx, edi", "xgetbv", "shl rdx, 32", "or rax, rdx", "ret")
        }
    };
}
