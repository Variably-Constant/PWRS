//! PowerShell providers: a Rust trait mapped to a generated
//! `NavigationCmdletProvider`.
//!
//! One Rust instance serves one drive. [`Provider::default_drives`]
//! and [`Provider::new_drive`] create instances, the managed drive
//! object carries the pointer, every item, container and content
//! operation runs against the current drive's instance with
//! `&mut self`, and `Remove-PSDrive` drops it. Path and validity
//! methods take no instance, since the engine asks them before a
//! drive is known. Each operation takes an `object[]` of arguments
//! and returns an `object[]` of results. A returned item is a row
//! `[path, value, is_container]`; a drive is a row
//! `[name, root, instance]`.

use crate::{ErrorCategory, FromPs, IntoPs, PsError, PsObject, PsResult};
use core::ffi::c_void;

/// One item a provider yields: its provider path, the object shown for
/// it, and whether it is a container (a directory-like node).
pub struct Item {
    pub path: String,
    pub value: PsObject,
    pub is_container: bool,
}

impl Item {
    pub fn leaf(path: impl Into<String>, value: PsObject) -> Item {
        Item { path: path.into(), value, is_container: false }
    }

    pub fn container(path: impl Into<String>, value: PsObject) -> Item {
        Item { path: path.into(), value, is_container: true }
    }
}

/// A drive a provider exposes, from `New-PSDrive` or as a default.
pub struct Drive {
    pub name: String,
    pub root: String,
}

/// Operation codes shared with the generated managed provider.
pub mod op {
    pub const IS_VALID_PATH: u32 = 0;
    pub const ITEM_EXISTS: u32 = 1;
    pub const IS_ITEM_CONTAINER: u32 = 2;
    pub const GET_ITEM: u32 = 3;
    pub const SET_ITEM: u32 = 4;
    pub const CLEAR_ITEM: u32 = 5;
    pub const GET_CHILD_ITEMS: u32 = 6;
    pub const GET_CHILD_NAMES: u32 = 7;
    pub const HAS_CHILD_ITEMS: u32 = 8;
    pub const NEW_ITEM: u32 = 9;
    pub const REMOVE_ITEM: u32 = 10;
    pub const RENAME_ITEM: u32 = 11;
    pub const COPY_ITEM: u32 = 12;
    pub const GET_CONTENT: u32 = 13;
    pub const SET_CONTENT: u32 = 14;
    pub const CLEAR_CONTENT: u32 = 15;
    pub const NEW_DRIVE: u32 = 16;
    pub const REMOVE_DRIVE: u32 = 17;
    pub const INIT_DEFAULT_DRIVES: u32 = 18;
    pub const MAKE_PATH: u32 = 19;
    pub const GET_PARENT_PATH: u32 = 20;
    pub const GET_CHILD_NAME: u32 = 21;
    pub const NORMALIZE_RELATIVE_PATH: u32 = 22;
    /// Frees a drive's instance without running `remove_drive`; the
    /// managed drive object's finalizer sends this for a drive that
    /// was never removed.
    pub const DROP_DRIVE: u32 = 23;
}

fn unsupported(what: &str) -> PsError {
    PsError::new(ErrorCategory::NotImplemented, "PwrsProviderUnsupported", format!("this provider does not implement {what}"))
}

fn unsupported_for(what: &str, path: &str) -> PsError {
    PsError::new(ErrorCategory::NotImplemented, "PwrsProviderUnsupported", format!("this provider does not implement {what} (path {path})"))
}

/// A PowerShell provider. One value of the implementing type serves
/// one drive: it is made by [`Provider::default_drives`] or
/// [`Provider::new_drive`], receives every operation on that drive
/// through `&mut self`, and is dropped after [`Provider::remove_drive`].
/// Methods default to "not supported"; a provider overrides what it
/// offers. `#[provider]` generates the managed
/// `NavigationCmdletProvider` that forwards to these.
///
/// Paths are provider paths (drive-qualified or drive-relative as the
/// engine passes them); a provider normalizes as it sees fit. The path
/// methods and `is_valid_path` are associated functions because the
/// engine asks them before a drive is known.
pub trait Provider: Send + 'static {
    // ---- drives ----
    /// The drives that exist when the module is imported, each with the
    /// instance that serves it.
    fn default_drives() -> PsResult<Vec<(Drive, Self)>>
    where
        Self: Sized,
    {
        Ok(Vec::new())
    }
    /// `New-PSDrive`: the drive as it will be registered (its root may
    /// differ from what the user gave) and the instance serving it.
    fn new_drive(name: &str, _root: &str) -> PsResult<(Drive, Self)>
    where
        Self: Sized,
    {
        Err(unsupported_for("new_drive", name))
    }
    /// `Remove-PSDrive`; the instance is dropped after this returns
    /// `Ok`, and kept with the drive after an `Err`.
    fn remove_drive(&mut self) -> PsResult<()> {
        Ok(())
    }

    // ---- paths, asked before a drive is known ----
    fn is_valid_path(_path: &str) -> bool {
        true
    }
    fn make_path(parent: &str, child: &str) -> PsResult<String> {
        Ok(default_make_path(parent, child))
    }
    fn get_parent_path(path: &str, root: &str) -> PsResult<String> {
        Ok(default_parent_path(path, root))
    }
    fn get_child_name(path: &str) -> PsResult<String> {
        Ok(child_name(path))
    }
    fn normalize_relative_path(path: &str, _base: &str) -> PsResult<String> {
        Ok(path.to_string())
    }

    // ---- items ----
    fn item_exists(&mut self, path: &str) -> PsResult<bool> {
        Err(unsupported_for("item_exists", path))
    }
    fn is_item_container(&mut self, path: &str) -> PsResult<bool> {
        Err(unsupported_for("is_item_container", path))
    }
    fn get_item(&mut self, path: &str) -> PsResult<Option<Item>> {
        Err(unsupported_for("get_item", path))
    }
    fn set_item(&mut self, _path: &str, _value: PsObject) -> PsResult<Option<Item>> {
        Err(unsupported("set_item"))
    }
    fn clear_item(&mut self, _path: &str) -> PsResult<()> {
        Err(unsupported("clear_item"))
    }

    // ---- containers ----
    fn get_child_items(&mut self, path: &str, _recurse: bool) -> PsResult<Vec<Item>> {
        Err(unsupported_for("get_child_items", path))
    }
    fn get_child_names(&mut self, path: &str) -> PsResult<Vec<String>> {
        Ok(self.get_child_items(path, false)?.into_iter().map(|i| child_name(&i.path)).collect())
    }
    fn has_child_items(&mut self, path: &str) -> PsResult<bool> {
        Ok(!self.get_child_items(path, false)?.is_empty())
    }
    fn new_item(&mut self, _path: &str, _item_type: &str, _value: PsObject) -> PsResult<Option<Item>> {
        Err(unsupported("new_item"))
    }
    fn remove_item(&mut self, _path: &str, _recurse: bool) -> PsResult<()> {
        Err(unsupported("remove_item"))
    }
    fn rename_item(&mut self, _path: &str, _new_name: &str) -> PsResult<Option<Item>> {
        Err(unsupported("rename_item"))
    }
    fn copy_item(&mut self, _path: &str, _dest: &str, _recurse: bool) -> PsResult<Option<Item>> {
        Err(unsupported("copy_item"))
    }

    // ---- content ----
    fn get_content(&mut self, path: &str) -> PsResult<Vec<PsObject>> {
        Err(unsupported_for("get_content", path))
    }
    fn set_content(&mut self, _path: &str, _content: Vec<PsObject>) -> PsResult<()> {
        Err(unsupported("set_content"))
    }
    fn clear_content(&mut self, _path: &str) -> PsResult<()> {
        Err(unsupported("clear_content"))
    }
}

/// The last segment of a `\`- or `/`-separated path.
pub fn child_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    match trimmed.rsplit(['\\', '/']).next() {
        Some(name) => name.to_string(),
        None => trimmed.to_string(),
    }
}

fn default_make_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        return child.to_string();
    }
    let sep = if parent.contains('/') && !parent.contains('\\') { '/' } else { '\\' };
    format!("{}{}{}", parent.trim_end_matches(['\\', '/']), sep, child.trim_start_matches(['\\', '/']))
}

fn default_parent_path(path: &str, root: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    match trimmed.rsplit_once(['\\', '/']) {
        Some((head, _)) if !head.is_empty() => head.to_string(),
        _ => root.to_string(),
    }
}

/// Runs one provider operation by op code against a drive's instance
/// (null for the operations that need none).
pub type ProviderDispatch = unsafe fn(*mut c_void, u32, &[PsObject]) -> PsResult<Vec<PsObject>>;

/// Registry entry; index is the provider id.
pub struct ProviderEntry {
    pub name: &'static str,
    pub descriptor: &'static str,
    pub dispatch: ProviderDispatch,
}

fn item_row(item: Item) -> PsResult<PsObject> {
    crate::convert::PsArray(vec![item.path.into_ps()?, item.value, item.is_container.into_ps()?]).into_ps()
}

/// `[name, root, instance]`; the pointer crosses as an `i64`.
fn drive_row(d: Drive, instance: *mut c_void) -> PsResult<PsObject> {
    crate::convert::PsArray(vec![d.name.into_ps()?, d.root.into_ps()?, (instance as usize as i64).into_ps()?]).into_ps()
}

fn arg_str(args: &[PsObject], i: usize) -> PsResult<String> {
    match args.get(i) {
        Some(o) => String::from_ps(o),
        None => Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsProviderArgs", format!("missing provider argument {i}"))),
    }
}

fn arg_bool(args: &[PsObject], i: usize) -> PsResult<bool> {
    match args.get(i) {
        Some(o) if !o.is_null() => bool::from_ps(o),
        _ => Ok(false),
    }
}

fn boxed<T: Provider>(instance: T) -> *mut c_void {
    Box::into_raw(Box::new(instance)) as *mut c_void
}

/// # Safety
/// `instance` is null or a live pointer to a `T` made by [`boxed`].
unsafe fn instance_mut<'a, T: Provider>(instance: *mut c_void) -> PsResult<&'a mut T> {
    if instance.is_null() {
        return Err(PsError::new(
            ErrorCategory::InvalidOperation,
            "PwrsProviderNoDrive",
            "the operation has no drive of this provider; use a drive-qualified path",
        ));
    }
    Ok(&mut *(instance as *mut T))
}

/// Dispatches one provider operation for concrete provider type `T`.
///
/// # Safety
/// `instance` is null or a live pointer to a `T` this function made
/// for a drive row; after `REMOVE_DRIVE` returns `Ok` or `DROP_DRIVE`
/// returns, the pointer is dangling.
pub unsafe fn dispatch<T: Provider>(instance: *mut c_void, op_code: u32, args: &[PsObject]) -> PsResult<Vec<PsObject>> {
    use op::*;
    match op_code {
        INIT_DEFAULT_DRIVES => T::default_drives()?.into_iter().map(|(d, p)| drive_row(d, boxed(p))).collect(),
        NEW_DRIVE => {
            let (drive, p) = T::new_drive(&arg_str(args, 0)?, &arg_str(args, 1)?)?;
            Ok(vec![drive_row(drive, boxed(p))?])
        }
        REMOVE_DRIVE => {
            instance_mut::<T>(instance)?.remove_drive()?;
            drop(Box::from_raw(instance as *mut T));
            Ok(Vec::new())
        }
        DROP_DRIVE => {
            if !instance.is_null() {
                drop(Box::from_raw(instance as *mut T));
            }
            Ok(Vec::new())
        }
        IS_VALID_PATH => Ok(vec![T::is_valid_path(&arg_str(args, 0)?).into_ps()?]),
        MAKE_PATH => Ok(vec![T::make_path(&arg_str(args, 0)?, &arg_str(args, 1)?)?.into_ps()?]),
        GET_PARENT_PATH => Ok(vec![T::get_parent_path(&arg_str(args, 0)?, &arg_str(args, 1)?)?.into_ps()?]),
        GET_CHILD_NAME => Ok(vec![T::get_child_name(&arg_str(args, 0)?)?.into_ps()?]),
        NORMALIZE_RELATIVE_PATH => Ok(vec![T::normalize_relative_path(&arg_str(args, 0)?, &arg_str(args, 1)?)?.into_ps()?]),
        item_op => drive_op(instance_mut::<T>(instance)?, item_op, args),
    }
}

/// The operations that run against a drive's instance.
fn drive_op<T: Provider>(p: &mut T, op_code: u32, args: &[PsObject]) -> PsResult<Vec<PsObject>> {
    use op::*;
    match op_code {
        ITEM_EXISTS => Ok(vec![p.item_exists(&arg_str(args, 0)?)?.into_ps()?]),
        IS_ITEM_CONTAINER => Ok(vec![p.is_item_container(&arg_str(args, 0)?)?.into_ps()?]),
        GET_ITEM => match p.get_item(&arg_str(args, 0)?)? {
            Some(item) => Ok(vec![item_row(item)?]),
            None => Ok(Vec::new()),
        },
        SET_ITEM => {
            let value = match args.get(1) {
                Some(o) => o.clone(),
                None => PsObject::null(),
            };
            match p.set_item(&arg_str(args, 0)?, value)? {
                Some(item) => Ok(vec![item_row(item)?]),
                None => Ok(Vec::new()),
            }
        }
        CLEAR_ITEM => {
            p.clear_item(&arg_str(args, 0)?)?;
            Ok(Vec::new())
        }
        GET_CHILD_ITEMS => {
            let rows = p.get_child_items(&arg_str(args, 0)?, arg_bool(args, 1)?)?;
            rows.into_iter().map(item_row).collect()
        }
        GET_CHILD_NAMES => {
            let names = p.get_child_names(&arg_str(args, 0)?)?;
            names.into_iter().map(|n| n.into_ps()).collect()
        }
        HAS_CHILD_ITEMS => Ok(vec![p.has_child_items(&arg_str(args, 0)?)?.into_ps()?]),
        NEW_ITEM => {
            let value = match args.get(2) {
                Some(o) => o.clone(),
                None => PsObject::null(),
            };
            match p.new_item(&arg_str(args, 0)?, &arg_str(args, 1)?, value)? {
                Some(item) => Ok(vec![item_row(item)?]),
                None => Ok(Vec::new()),
            }
        }
        REMOVE_ITEM => {
            p.remove_item(&arg_str(args, 0)?, arg_bool(args, 1)?)?;
            Ok(Vec::new())
        }
        RENAME_ITEM => match p.rename_item(&arg_str(args, 0)?, &arg_str(args, 1)?)? {
            Some(item) => Ok(vec![item_row(item)?]),
            None => Ok(Vec::new()),
        },
        COPY_ITEM => match p.copy_item(&arg_str(args, 0)?, &arg_str(args, 1)?, arg_bool(args, 2)?)? {
            Some(item) => Ok(vec![item_row(item)?]),
            None => Ok(Vec::new()),
        },
        GET_CONTENT => p.get_content(&arg_str(args, 0)?),
        SET_CONTENT => {
            let content = match args.get(1..) {
                Some(rest) => rest.to_vec(),
                None => Vec::new(),
            };
            p.set_content(&arg_str(args, 0)?, content)?;
            Ok(Vec::new())
        }
        CLEAR_CONTENT => {
            p.clear_content(&arg_str(args, 0)?)?;
            Ok(Vec::new())
        }
        other => Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsProviderOp", format!("unknown provider op {other}"))),
    }
}

/// Compile-time metadata `#[provider]` generates.
pub trait ProviderMeta {
    const NAME: &'static str;
    const DESCRIPTOR: &'static str;
}
