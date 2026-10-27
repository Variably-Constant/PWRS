//! PowerShell providers: a Rust trait mapped to a generated
//! `NavigationCmdletProvider`.
//!
//! Each drive has one Rust instance. [`Provider::default_drives`]
//! and [`Provider::new_drive`] create instances, the managed drive
//! object stores the pointer, every item, container and content
//! operation runs against the current drive's instance with
//! `&mut self`, and `Remove-PSDrive` drops it. Path and validity
//! methods take no instance, since the engine calls them before it has
//! a drive to run them against. Each operation takes an `object[]` of
//! arguments and returns an `object[]` of results. A returned item is a
//! row `[path, value, is_container]`; a drive is a row
//! `[name, root, instance]`. Properties are cells
//! `[name, value, name, value, ...]`, as arguments and as results, and a
//! security descriptor is `[sddl, kind]`.

use crate::{ErrorCategory, FromPs, IntoPs, PsCredential, PsError, PsObject, PsResult};
use core::ffi::c_void;
use std::cell::RefCell;

/// The values of one provider call besides its own arguments: those
/// PowerShell's own providers read from `this.Filter`, `this.Include`,
/// `this.Exclude`, `this.Force` and `this.Credential` while a call runs.
/// [`call`] returns the current call's.
#[derive(Clone, Debug, Default)]
pub struct Call {
    /// The text of `-Filter`, which PowerShell never reads itself. It
    /// reaches a provider that declares the `Filter` capability, and
    /// PowerShell refuses `-Filter` for one that does not.
    pub filter: Option<String>,
    /// The `-Include` patterns. PowerShell applies them itself, whether or
    /// not the provider declares the `Include` capability; a provider can
    /// read them too, such as to skip work.
    pub include: Vec<String>,
    /// The `-Exclude` patterns, which PowerShell applies itself as it does
    /// `-Include`.
    pub exclude: Vec<String>,
    /// Whether `-Force` was given.
    pub force: bool,
    /// The `-Credential` given, for a provider that declares the
    /// `Credentials` capability; PowerShell refuses `-Credential` for one
    /// that does not. `New-PSDrive -Credential` reaches `new_drive` this
    /// way, and later calls on that drive have no credential.
    pub credential: Option<PsCredential>,
}

thread_local! {
    /// The values of the provider operation running on this thread.
    static CURRENT: RefCell<Call> = RefCell::new(Call::default());
}

/// The values of the provider call running on this thread: its filter,
/// include and exclude patterns, `-Force` and credential. Outside
/// a provider call, and on a thread the call did not start on, every
/// field is empty.
pub fn call() -> Call {
    CURRENT.with(|current| current.borrow().clone())
}

/// Makes a call the current one until it is dropped, then puts back the
/// call before it, so a call that runs inside another has its own values.
struct Entered(Option<Call>);

impl Entered {
    fn new(call: Call) -> Entered {
        Entered(Some(CURRENT.with(|current| current.replace(call))))
    }
}

impl Drop for Entered {
    fn drop(&mut self) {
        if let Some(outer) = self.0.take() {
            CURRENT.with(|current| {
                current.replace(outer);
            });
        }
    }
}

/// How many arguments every operation starts with: the call's filter,
/// include patterns, exclude patterns, `-Force` and credential, before the
/// operation's own.
const CONTEXT_ARGS: usize = 5;

/// The call the first [`CONTEXT_ARGS`] arguments of an operation give.
fn read_call(context: &[PsObject]) -> PsResult<Call> {
    let strings = |o: &PsObject| if o.is_null() { Ok(Vec::new()) } else { Vec::<String>::from_ps(o) };
    Ok(Call {
        filter: Option::<String>::from_ps(&context[0])?,
        include: strings(&context[1])?,
        exclude: strings(&context[2])?,
        force: arg_bool(context, 3)?,
        credential: Option::<PsCredential>::from_ps(&context[4])?,
    })
}

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

/// The parts of a security descriptor one call covers. `Get-Acl` covers
/// the owner, the group and the access list, and the audit list too
/// when given `-Audit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecuritySections {
    pub owner: bool,
    pub group: bool,
    pub access: bool,
    pub audit: bool,
}

impl SecuritySections {
    /// Every part.
    pub const ALL: SecuritySections = SecuritySections { owner: true, group: true, access: true, audit: true };

    /// The parts set in .NET's `AccessControlSections` flags: audit 1,
    /// access 2, owner 4, group 8.
    fn from_flags(flags: i32) -> SecuritySections {
        SecuritySections { audit: flags & 1 != 0, access: flags & 2 != 0, owner: flags & 4 != 0, group: flags & 8 != 0 }
    }
}

/// What a security descriptor describes. The kind sets which object
/// `Get-Acl` returns, and so whether the rights show by name: PowerShell
/// shows them by name only for files, directories and registry keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityKind {
    /// A `FileSecurity`, whose rights read as file rights such as
    /// `FullControl` or `Read`.
    File,
    /// A `DirectorySecurity`, with file rights.
    Directory,
    /// A `RegistrySecurity`, with registry rights.
    RegistryKey,
    /// A `Pwrs.ItemSecurity`, whose rules give the rights as a number,
    /// since what each bit means is the provider's own.
    Other,
}

impl SecurityKind {
    /// The code the managed provider reads.
    fn code(self) -> i32 {
        match self {
            SecurityKind::File => 0,
            SecurityKind::Directory => 1,
            SecurityKind::RegistryKey => 2,
            SecurityKind::Other => 3,
        }
    }
}

/// A security descriptor as SDDL text, the one-line form `Get-Acl`
/// shows as `Sddl`, and the kind of object it describes.
pub struct SecurityDescriptor {
    pub sddl: String,
    pub kind: SecurityKind,
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
    pub const WRITE_CONTENT: u32 = 14;
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
    pub const GET_PROPERTY: u32 = 24;
    pub const SET_PROPERTY: u32 = 25;
    pub const CLEAR_PROPERTY: u32 = 26;
    pub const NEW_PROPERTY: u32 = 27;
    pub const REMOVE_PROPERTY: u32 = 28;
    pub const RENAME_PROPERTY: u32 = 29;
    pub const COPY_PROPERTY: u32 = 30;
    pub const MOVE_PROPERTY: u32 = 31;
    pub const GET_SECURITY_DESCRIPTOR: u32 = 32;
    pub const SET_SECURITY_DESCRIPTOR: u32 = 33;
    /// The operations between two drives of one provider take the
    /// destination drive's instance as their second argument.
    pub const COPY_ITEM_TO: u32 = 34;
    pub const COPY_PROPERTY_TO: u32 = 35;
    pub const MOVE_PROPERTY_TO: u32 = 36;
    pub const MOVE_ITEM: u32 = 37;
    /// Between two drives, as the operations above.
    pub const MOVE_ITEM_TO: u32 = 38;
    pub const INVOKE_DEFAULT_ACTION: u32 = 39;
    pub const EXPAND_PATH: u32 = 40;
}

fn unsupported(what: &str) -> PsError {
    PsError::new(ErrorCategory::NotImplemented, "PwrsProviderUnsupported", format!("this provider does not implement {what}"))
}

fn unsupported_for(what: &str, path: &str) -> PsError {
    PsError::new(ErrorCategory::NotImplemented, "PwrsProviderUnsupported", format!("this provider does not implement {what} (path {path})"))
}

/// A PowerShell provider. Each drive has one value of the implementing
/// type: it is made by [`Provider::default_drives`] or
/// [`Provider::new_drive`], receives every operation on that drive
/// through `&mut self`, and is dropped after [`Provider::remove_drive`].
/// Methods default to "not supported"; a provider overrides what it
/// offers. `#[provider]` generates the managed
/// `NavigationCmdletProvider` that forwards to these.
///
/// A path reaches these methods without its drive, `docs\a.txt` for
/// `mem:\docs\a.txt`, with `\`, `/` or both as separators, so a provider
/// normalizes paths itself. A path that names no drive of the provider
/// comes as PowerShell passed it. The path methods and `is_valid_path`
/// are associated functions because the engine calls them before it has
/// a drive to run them against.
///
/// A method that changes something runs only once the user has confirmed
/// the change under `-Confirm`, and never under `-WhatIf`: the managed
/// provider, or the cmdlet itself, calls `ShouldProcess`, PowerShell's
/// check for both, first.
pub trait Provider: Send + 'static {
    // ---- drives ----
    /// The drives that exist when the module is imported, each with its
    /// instance.
    fn default_drives() -> PsResult<Vec<(Drive, Self)>>
    where
        Self: Sized,
    {
        Ok(Vec::new())
    }
    /// `New-PSDrive`: the drive as it will be registered (its root may
    /// differ from what the user gave) and its instance.
    fn new_drive(name: &str, _root: &str) -> PsResult<(Drive, Self)>
    where
        Self: Sized,
    {
        Err(unsupported_for("new_drive", name))
    }
    /// `Remove-PSDrive`; the instance is dropped after this returns
    /// `Ok`, and stays with the drive after an `Err`.
    fn remove_drive(&mut self) -> PsResult<()> {
        Ok(())
    }

    // ---- paths, called before there is a drive ----
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
    /// The path `path` names, written relative to `base`, as
    /// `Resolve-Path -Relative` shows it, for a provider with rules of its
    /// own. With `None`, the default, PowerShell works out the relative
    /// path itself from `get_parent_path`, `get_child_name` and
    /// `make_path`.
    fn normalize_relative_path(_path: &str, _base: &str) -> PsResult<Option<String>> {
        Ok(None)
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
    /// `Invoke-Item`: runs the item's default action, whatever this
    /// provider takes that to be, as opening a file is for FileSystem.
    fn invoke_default_action(&mut self, path: &str) -> PsResult<()> {
        Err(unsupported_for("invoke_default_action", path))
    }
    /// The paths a path with wildcards names, for a provider that declares
    /// the `ExpandWildcards` capability; PowerShell then runs the command
    /// once for each. Each path returned must exist, and the matching
    /// should follow PowerShell's wildcard rules. PowerShell then passes
    /// every path without wildcards on as it was typed, so the provider
    /// applies `-Include` and `-Exclude` to the paths it returns, and
    /// reports a path that is not there, itself. The default returns the
    /// path as given.
    fn expand_path(&mut self, path: &str) -> PsResult<Vec<String>> {
        Ok(vec![path.to_string()])
    }

    // ---- containers ----
    /// The children of the container at `path`, and with `recurse` every
    /// item below it. A provider that declares `Filter` returns the items
    /// [`call`]'s filter selects, and still searches a container the
    /// filter leaves out.
    fn get_child_items(&mut self, path: &str, _recurse: bool) -> PsResult<Vec<Item>> {
        Err(unsupported_for("get_child_items", path))
    }
    /// The names of the children of the container at `path`, by default
    /// those of `get_child_items`. With a filter, these are the names it
    /// selects; when PowerShell walks into every container whatever the
    /// filter, the managed provider adds the ones they leave out.
    fn get_child_names(&mut self, path: &str) -> PsResult<Vec<String>> {
        Ok(self.get_child_items(path, false)?.into_iter().map(|i| child_name(&i.path)).collect())
    }
    /// Whether the container at `path` has children. It runs without the
    /// call's filter and patterns, as on a FileSystem drive.
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
    /// `Copy-Item` to another drive of this provider: copies the item at
    /// `path` on this drive to `dest` on `dest_drive`, and returns the
    /// copy.
    fn copy_item_to(&mut self, path: &str, _dest_drive: &mut Self, _dest: &str, _recurse: bool) -> PsResult<Option<Item>>
    where
        Self: Sized,
    {
        Err(unsupported_for("copy_item_to", path))
    }
    /// `Move-Item`: moves the item at `path` to `dest`, and returns it at
    /// its new path.
    fn move_item(&mut self, path: &str, _dest: &str) -> PsResult<Option<Item>> {
        Err(unsupported_for("move_item", path))
    }
    /// `Move-Item` to another drive of this provider: moves the item at
    /// `path` on this drive to `dest` on `dest_drive`, and returns it
    /// there.
    fn move_item_to(&mut self, path: &str, _dest_drive: &mut Self, _dest: &str) -> PsResult<Option<Item>>
    where
        Self: Sized,
    {
        Err(unsupported_for("move_item_to", path))
    }

    // ---- content ----
    fn get_content(&mut self, path: &str) -> PsResult<Vec<PsObject>> {
        Err(unsupported_for("get_content", path))
    }
    /// `Set-Content` and `Add-Content`: appends `lines`, the objects of
    /// one write, to the item at `path`, and makes the item when there is
    /// none. A command writes its input in one or more batches, each a
    /// call. `Set-Content` empties an item that exists through
    /// `clear_content` before its first batch, so a provider whose items
    /// `Set-Content` replaces implements both.
    fn write_content(&mut self, _path: &str, _lines: Vec<PsObject>) -> PsResult<()> {
        Err(unsupported("write_content"))
    }
    /// `Clear-Content`, and `Set-Content` on an item that exists.
    fn clear_content(&mut self, _path: &str) -> PsResult<()> {
        Err(unsupported("clear_content"))
    }

    // ---- item properties ----
    // Properties travel as name and value pairs. Each method that changes
    // a property returns the properties it changed as they now are. The
    // managed provider writes them as one object, which `-PassThru` shows,
    // and writes nothing for no pairs.

    /// `Get-ItemProperty`: the properties `names` names on the item at
    /// `path`, or all of them when `names` is empty.
    fn get_property(&mut self, path: &str, _names: &[String]) -> PsResult<Vec<(String, PsObject)>> {
        Err(unsupported_for("get_property", path))
    }
    /// `Set-ItemProperty`: sets each pair on the item at `path`.
    fn set_property(&mut self, path: &str, _values: &[(String, PsObject)]) -> PsResult<Vec<(String, PsObject)>> {
        Err(unsupported_for("set_property", path))
    }
    /// `Clear-ItemProperty`: clears the value of each property `names`
    /// names on the item at `path`.
    fn clear_property(&mut self, path: &str, _names: &[String]) -> PsResult<Vec<(String, PsObject)>> {
        Err(unsupported_for("clear_property", path))
    }
    /// `New-ItemProperty`: adds the property `name` with `value` to the
    /// item at `path`. `type_name` is what `-PropertyType` gave, and is
    /// empty when it was not given. [`call`]'s `force` is whether `-Force`
    /// was given; on the registry drive it lets the call replace a
    /// property that already exists.
    fn new_property(&mut self, path: &str, _name: &str, _type_name: &str, _value: PsObject) -> PsResult<Vec<(String, PsObject)>> {
        Err(unsupported_for("new_property", path))
    }
    /// `Remove-ItemProperty`: removes the property `name` from the item at
    /// `path`.
    fn remove_property(&mut self, path: &str, _name: &str) -> PsResult<()> {
        Err(unsupported_for("remove_property", path))
    }
    /// `Rename-ItemProperty`: gives the property `name` on the item at
    /// `path` the name `new_name`.
    fn rename_property(&mut self, path: &str, _name: &str, _new_name: &str) -> PsResult<Vec<(String, PsObject)>> {
        Err(unsupported_for("rename_property", path))
    }
    /// `Copy-ItemProperty`: copies the property `name` of the item at
    /// `path` to the property `dest_name` of the item at `dest_path`, and
    /// returns the copy.
    fn copy_property(&mut self, path: &str, _name: &str, _dest_path: &str, _dest_name: &str) -> PsResult<Vec<(String, PsObject)>> {
        Err(unsupported_for("copy_property", path))
    }
    /// `Move-ItemProperty`: moves the property `name` of the item at
    /// `path` to the property `dest_name` of the item at `dest_path`, and
    /// returns it at its new place.
    fn move_property(&mut self, path: &str, _name: &str, _dest_path: &str, _dest_name: &str) -> PsResult<Vec<(String, PsObject)>> {
        Err(unsupported_for("move_property", path))
    }
    /// `Copy-ItemProperty` to an item on another drive of this provider.
    fn copy_property_to(&mut self, path: &str, _name: &str, _dest_drive: &mut Self, _dest_path: &str, _dest_name: &str) -> PsResult<Vec<(String, PsObject)>>
    where
        Self: Sized,
    {
        Err(unsupported_for("copy_property_to", path))
    }
    /// `Move-ItemProperty` to an item on another drive of this provider.
    fn move_property_to(&mut self, path: &str, _name: &str, _dest_drive: &mut Self, _dest_path: &str, _dest_name: &str) -> PsResult<Vec<(String, PsObject)>>
    where
        Self: Sized,
    {
        Err(unsupported_for("move_property_to", path))
    }

    // ---- security descriptors ----
    /// `Get-Acl`: the security descriptor of the item at `path`.
    /// `sections` lists the parts the call covers; the SDDL text may
    /// leave the others out.
    fn get_security_descriptor(&mut self, path: &str, _sections: SecuritySections) -> PsResult<SecurityDescriptor> {
        Err(unsupported_for("get_security_descriptor", path))
    }
    /// `Set-Acl`: sets the security descriptor of the item at `path`. The
    /// SDDL text includes each part the caller's descriptor has.
    fn set_security_descriptor(&mut self, path: &str, _sddl: &str) -> PsResult<()> {
        Err(unsupported_for("set_security_descriptor", path))
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

/// `child` joined to `parent` with the separator the parent has, or, when
/// it has none or both kinds, the one this system writes paths with: `\`
/// on Windows and `/` elsewhere, as PowerShell does.
fn default_make_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        return child.to_string();
    }
    let sep = match (parent.contains('\\'), parent.contains('/')) {
        (true, false) => '\\',
        (false, true) => '/',
        _neither_or_both => std::path::MAIN_SEPARATOR,
    };
    format!("{}{}{}", parent.trim_end_matches(['\\', '/']), sep, child.trim_start_matches(['\\', '/']))
}

/// Everything before the last separator, trailing separators ignored. A
/// path whose only separator is its first character has that separator
/// for a parent, as in PowerShell's own rule, so `\docs` gives `\`; a
/// path with no separator has `root` for a parent.
fn default_parent_path(path: &str, root: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    match trimmed.rfind(['\\', '/']) {
        Some(0) => trimmed[..1].to_string(),
        Some(i) => trimmed[..i].to_string(),
        None => root.to_string(),
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

/// `[name, root, instance]`; the pointer is passed as an `i64`.
fn drive_row(d: Drive, instance: *mut c_void) -> PsResult<PsObject> {
    crate::convert::PsArray(vec![d.name.into_ps()?, d.root.into_ps()?, (instance as usize as i64).into_ps()?]).into_ps()
}

fn arg_str(args: &[PsObject], i: usize) -> PsResult<String> {
    match args.get(i) {
        Some(o) => String::from_ps(o),
        None => Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsProviderArgs", format!("missing provider argument {i}"))),
    }
}

/// Every argument from `from` on, as strings.
fn arg_strs(args: &[PsObject], from: usize) -> PsResult<Vec<String>> {
    args.get(from..).unwrap_or(&[]).iter().map(String::from_ps).collect()
}

/// Every argument from `from` on, read as name and value pairs.
fn arg_pairs(args: &[PsObject], from: usize) -> PsResult<Vec<(String, PsObject)>> {
    let (pairs, unpaired) = args.get(from..).unwrap_or(&[]).as_chunks::<2>();
    if !unpaired.is_empty() {
        return Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsProviderArgs", "a property argument has a name and no value"));
    }
    pairs.iter().map(|[name, value]| Ok((String::from_ps(name)?, value.clone()))).collect()
}

/// Name and value pairs as the cells `[name, value, name, value, ...]`.
fn pair_cells(pairs: Vec<(String, PsObject)>) -> PsResult<Vec<PsObject>> {
    let mut cells = Vec::new();
    cells.try_reserve(pairs.len() * 2)?;
    for (name, value) in pairs {
        cells.push(name.into_ps()?);
        cells.push(value);
    }
    Ok(cells)
}

/// The argument at `i`, or `$null` when there is none.
fn arg_object(args: &[PsObject], i: usize) -> PsObject {
    match args.get(i) {
        Some(o) => o.clone(),
        None => PsObject::null(),
    }
}

fn arg_i32(args: &[PsObject], i: usize) -> PsResult<i32> {
    match args.get(i) {
        Some(o) => i32::from_ps(o),
        None => Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsProviderArgs", format!("missing provider argument {i}"))),
    }
}

/// A drive's instance given as an argument, an `i64` as in a drive row.
fn arg_instance(args: &[PsObject], i: usize) -> PsResult<*mut c_void> {
    match args.get(i) {
        Some(o) => Ok(i64::from_ps(o)? as usize as *mut c_void),
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
    Ok(unsafe { &mut *(instance as *mut T) })
}

/// Dispatches one provider operation for concrete provider type `T`. The
/// first [`CONTEXT_ARGS`] arguments are the call's, which [`call`]
/// returns while the operation runs; the rest are the operation's own.
///
/// # Safety
/// `instance` is null or a live pointer to a `T` this function made
/// for a drive row; after `REMOVE_DRIVE` returns `Ok` or `DROP_DRIVE`
/// returns, the pointer is dangling. For an operation between two
/// drives, the operation's second argument is the other drive's
/// instance, under the same terms.
pub unsafe fn dispatch<T: Provider>(instance: *mut c_void, op_code: u32, args: &[PsObject]) -> PsResult<Vec<PsObject>> {
    let Some((context, own)) = args.split_at_checked(CONTEXT_ARGS) else {
        return Err(PsError::new(
            ErrorCategory::InvalidArgument,
            "PwrsProviderArgs",
            format!("a provider operation needs its call's {CONTEXT_ARGS} arguments before its own, and it got {}", args.len()),
        ));
    };
    let _entered = Entered::new(read_call(context)?);
    unsafe { run::<T>(instance, op_code, own) }
}

/// Runs one operation on its own arguments, under [`dispatch`]'s terms.
///
/// # Safety
/// As for [`dispatch`].
unsafe fn run<T: Provider>(instance: *mut c_void, op_code: u32, args: &[PsObject]) -> PsResult<Vec<PsObject>> {
    use op::*;
    match op_code {
        INIT_DEFAULT_DRIVES => T::default_drives()?.into_iter().map(|(d, p)| drive_row(d, boxed(p))).collect(),
        NEW_DRIVE => {
            let (drive, p) = T::new_drive(&arg_str(args, 0)?, &arg_str(args, 1)?)?;
            Ok(vec![drive_row(drive, boxed(p))?])
        }
        REMOVE_DRIVE => {
            unsafe { instance_mut::<T>(instance) }?.remove_drive()?;
            drop(unsafe { Box::from_raw(instance as *mut T) });
            Ok(Vec::new())
        }
        DROP_DRIVE => {
            if !instance.is_null() {
                drop(unsafe { Box::from_raw(instance as *mut T) });
            }
            Ok(Vec::new())
        }
        IS_VALID_PATH => Ok(vec![T::is_valid_path(&arg_str(args, 0)?).into_ps()?]),
        MAKE_PATH => Ok(vec![T::make_path(&arg_str(args, 0)?, &arg_str(args, 1)?)?.into_ps()?]),
        GET_PARENT_PATH => Ok(vec![T::get_parent_path(&arg_str(args, 0)?, &arg_str(args, 1)?)?.into_ps()?]),
        GET_CHILD_NAME => Ok(vec![T::get_child_name(&arg_str(args, 0)?)?.into_ps()?]),
        NORMALIZE_RELATIVE_PATH => match T::normalize_relative_path(&arg_str(args, 0)?, &arg_str(args, 1)?)? {
            Some(path) => Ok(vec![path.into_ps()?]),
            None => Ok(Vec::new()),
        },
        COPY_ITEM_TO | COPY_PROPERTY_TO | MOVE_PROPERTY_TO | MOVE_ITEM_TO => {
            let dest = arg_instance(args, 1)?;
            if dest == instance {
                return Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsProviderArgs", "an operation between two drives names one drive as both"));
            }
            let from = unsafe { instance_mut::<T>(instance) }?;
            let to = unsafe { instance_mut::<T>(dest) }?;
            between_drives(from, to, op_code, args)
        }
        item_op => drive_op(unsafe { instance_mut::<T>(instance) }?, item_op, args),
    }
}

/// The operations between two drives of the provider: `from` is the
/// source path's drive and `to` the destination's, whose instance is the
/// second argument.
fn between_drives<T: Provider>(from: &mut T, to: &mut T, op_code: u32, args: &[PsObject]) -> PsResult<Vec<PsObject>> {
    use op::*;
    match op_code {
        COPY_ITEM_TO => match from.copy_item_to(&arg_str(args, 0)?, to, &arg_str(args, 2)?, arg_bool(args, 3)?)? {
            Some(item) => Ok(vec![item_row(item)?]),
            None => Ok(Vec::new()),
        },
        COPY_PROPERTY_TO => pair_cells(from.copy_property_to(&arg_str(args, 0)?, &arg_str(args, 2)?, to, &arg_str(args, 3)?, &arg_str(args, 4)?)?),
        MOVE_PROPERTY_TO => pair_cells(from.move_property_to(&arg_str(args, 0)?, &arg_str(args, 2)?, to, &arg_str(args, 3)?, &arg_str(args, 4)?)?),
        MOVE_ITEM_TO => match from.move_item_to(&arg_str(args, 0)?, to, &arg_str(args, 2)?)? {
            Some(item) => Ok(vec![item_row(item)?]),
            None => Ok(Vec::new()),
        },
        other => Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsProviderOp", format!("unknown provider op {other}"))),
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
        INVOKE_DEFAULT_ACTION => {
            p.invoke_default_action(&arg_str(args, 0)?)?;
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
        MOVE_ITEM => match p.move_item(&arg_str(args, 0)?, &arg_str(args, 1)?)? {
            Some(item) => Ok(vec![item_row(item)?]),
            None => Ok(Vec::new()),
        },
        GET_CONTENT => p.get_content(&arg_str(args, 0)?),
        WRITE_CONTENT => {
            let lines = match args.get(1..) {
                Some(rest) => rest.to_vec(),
                None => Vec::new(),
            };
            p.write_content(&arg_str(args, 0)?, lines)?;
            Ok(Vec::new())
        }
        CLEAR_CONTENT => {
            p.clear_content(&arg_str(args, 0)?)?;
            Ok(Vec::new())
        }
        GET_PROPERTY => pair_cells(p.get_property(&arg_str(args, 0)?, &arg_strs(args, 1)?)?),
        SET_PROPERTY => pair_cells(p.set_property(&arg_str(args, 0)?, &arg_pairs(args, 1)?)?),
        CLEAR_PROPERTY => pair_cells(p.clear_property(&arg_str(args, 0)?, &arg_strs(args, 1)?)?),
        NEW_PROPERTY => pair_cells(p.new_property(&arg_str(args, 0)?, &arg_str(args, 1)?, &arg_str(args, 2)?, arg_object(args, 3))?),
        EXPAND_PATH => p.expand_path(&arg_str(args, 0)?)?.into_iter().map(|path| path.into_ps()).collect(),
        REMOVE_PROPERTY => {
            p.remove_property(&arg_str(args, 0)?, &arg_str(args, 1)?)?;
            Ok(Vec::new())
        }
        RENAME_PROPERTY => pair_cells(p.rename_property(&arg_str(args, 0)?, &arg_str(args, 1)?, &arg_str(args, 2)?)?),
        COPY_PROPERTY => pair_cells(p.copy_property(&arg_str(args, 0)?, &arg_str(args, 1)?, &arg_str(args, 2)?, &arg_str(args, 3)?)?),
        MOVE_PROPERTY => pair_cells(p.move_property(&arg_str(args, 0)?, &arg_str(args, 1)?, &arg_str(args, 2)?, &arg_str(args, 3)?)?),
        GET_SECURITY_DESCRIPTOR => {
            let sd = p.get_security_descriptor(&arg_str(args, 0)?, SecuritySections::from_flags(arg_i32(args, 1)?))?;
            Ok(vec![sd.sddl.into_ps()?, sd.kind.code().into_ps()?])
        }
        SET_SECURITY_DESCRIPTOR => {
            p.set_security_descriptor(&arg_str(args, 0)?, &arg_str(args, 1)?)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_read_each_bit_of_dotnets_flags() {
        let none = SecuritySections { owner: false, group: false, access: false, audit: false };
        assert_eq!(SecuritySections::from_flags(0), none);
        assert_eq!(SecuritySections::from_flags(1), SecuritySections { audit: true, ..none });
        assert_eq!(SecuritySections::from_flags(2), SecuritySections { access: true, ..none });
        assert_eq!(SecuritySections::from_flags(4), SecuritySections { owner: true, ..none });
        assert_eq!(SecuritySections::from_flags(8), SecuritySections { group: true, ..none });
        assert_eq!(SecuritySections::from_flags(14), SecuritySections { audit: false, ..SecuritySections::ALL });
        assert_eq!(SecuritySections::from_flags(15), SecuritySections::ALL);
    }

    #[test]
    fn a_parent_ends_at_a_leading_separator_and_falls_back_to_the_root() {
        assert_eq!(default_parent_path(r"docs\a.txt", ""), "docs");
        assert_eq!(default_parent_path(r"\docs\a.txt", ""), r"\docs");
        assert_eq!(default_parent_path(r"\docs", ""), r"\");
        assert_eq!(default_parent_path("/docs/", ""), "/");
        assert_eq!(default_parent_path("docs", ""), "");
        assert_eq!(default_parent_path("docs", "top"), "top");
        assert_eq!(default_parent_path(r"a\\b", ""), r"a\");
    }

    #[test]
    fn a_join_takes_the_parents_separator_or_the_systems() {
        assert_eq!(default_make_path(r"docs\sub", "a.txt"), r"docs\sub\a.txt");
        assert_eq!(default_make_path("docs/sub", "a.txt"), "docs/sub/a.txt");
        assert_eq!(default_make_path("docs", "a.txt"), format!("docs{}a.txt", std::path::MAIN_SEPARATOR));
        assert_eq!(default_make_path(r"docs\sub/deep", "a.txt"), format!(r"docs\sub/deep{}a.txt", std::path::MAIN_SEPARATOR));
        assert_eq!(default_make_path(r"docs\", r"\a.txt"), r"docs\a.txt");
        assert_eq!(default_make_path("", "a.txt"), "a.txt");
    }

    #[test]
    fn a_call_is_current_while_it_runs_and_one_inside_it_has_its_own() {
        assert!(call().filter.is_none());
        {
            let _outer = Entered::new(Call { filter: Some("outer".into()), force: true, ..Call::default() });
            assert_eq!(call().filter.as_deref(), Some("outer"));
            {
                let _inner = Entered::new(Call { include: vec!["*.txt".into()], ..Call::default() });
                assert!(call().filter.is_none());
                assert_eq!(call().include, ["*.txt"]);
                assert!(!call().force);
            }
            assert_eq!(call().filter.as_deref(), Some("outer"));
            assert!(call().force);
        }
        assert!(call().filter.is_none());
        assert!(!call().force);
    }

    #[test]
    fn a_calls_values_come_from_its_first_five_arguments() {
        use crate::testing::{self, Value};
        let _host = testing::install();
        let credential = Value::PsObject(
            "System.Management.Automation.PSCredential".into(),
            vec![("UserName".into(), Value::Str("ada".into())), ("Password".into(), Value::Secure("hunter2".encode_utf16().collect()))],
        );
        let context = [
            "*.txt".into_ps().expect("filter"),
            testing::object(Value::Array(vec![Value::Str("a*".into())])),
            testing::object(Value::Array(Vec::new())),
            true.into_ps().expect("force"),
            testing::object(credential),
        ];
        let given = read_call(&context).expect("a call");
        assert_eq!(given.filter.as_deref(), Some("*.txt"));
        assert_eq!(given.include, ["a*"]);
        assert!(given.exclude.is_empty());
        assert!(given.force);
        assert_eq!(given.credential.expect("a credential").user_name, "ada");
        let nothing = [PsObject::null(), PsObject::null(), PsObject::null(), PsObject::null(), PsObject::null()];
        let empty = read_call(&nothing).expect("an empty call");
        assert!(empty.filter.is_none() && empty.include.is_empty() && empty.exclude.is_empty() && !empty.force && empty.credential.is_none());
    }

    /// A provider whose one method returns whether its call had `-Force`.
    struct ForceReader;

    impl Provider for ForceReader {
        fn is_valid_path(_path: &str) -> bool {
            call().force
        }
    }

    #[test]
    fn an_operation_reads_its_calls_values_and_needs_them() {
        let _host = crate::testing::install();
        let forced = [PsObject::null(), PsObject::null(), PsObject::null(), true.into_ps().expect("force"), PsObject::null(), "a".into_ps().expect("path")];
        let rows = unsafe { dispatch::<ForceReader>(core::ptr::null_mut(), op::IS_VALID_PATH, &forced) }.expect("the operation runs");
        assert!(bool::from_ps(&rows[0]).expect("a bool"));
        assert!(!call().force);
        let refused = unsafe { dispatch::<ForceReader>(core::ptr::null_mut(), op::IS_VALID_PATH, &forced[3..]) }.expect_err("too few arguments");
        assert!(refused.to_string().contains("needs its call's 5 arguments before its own, and it got 3"), "{refused}");
    }

    #[test]
    fn each_kind_has_the_code_the_managed_provider_reads() {
        let codes: Vec<i32> = [SecurityKind::File, SecurityKind::Directory, SecurityKind::RegistryKey, SecurityKind::Other].iter().map(|k| k.code()).collect();
        assert_eq!(codes, [0, 1, 2, 3]);
    }
}
