//! Two in-memory providers. MemFs is a filesystem: `New-PSDrive
//! -PSProvider MemFs`, then `Get-ChildItem`, `New-Item`, `Set-Content`,
//! `Get-Content`, `Remove-Item`, `Rename-Item`, `Copy-Item`, `Move-Item`
//! and `Invoke-Item` against `mem:`, with a `-Filter` that matches names or
//! a property's value. Every item has properties of its own, which the
//! `*-ItemProperty` cmdlets read and change, and a security descriptor,
//! which `Get-Acl` and `Set-Acl` read and change. Every drive is its own
//! tree, on that drive's own instance. MemNotes is a list of notes on
//! `notes:` that resolves the wildcards in a path itself.

use pwrs::prelude::*;
use std::collections::BTreeMap;

#[derive(Clone)]
enum Kind {
    Dir,
    File(String),
}

/// One item: what it is, the properties given to it, in the order each
/// was first set, and its security descriptor as SDDL text.
#[derive(Clone)]
struct Node {
    kind: Kind,
    properties: Vec<(String, PsObject)>,
    sddl: String,
}

/// A new item's security descriptor: the Administrators group owns it
/// and has full control, everyone can read it, and a failed write by
/// anyone is audited.
const NEW_SDDL: &str = "O:BAG:BAD:(A;;FA;;;BA)(A;;FR;;;WD)S:(AU;FA;FW;;;WD)";

impl Node {
    fn new(kind: Kind) -> Node {
        Node { kind, properties: Vec::new(), sddl: NEW_SDDL.to_string() }
    }
}

/// Normalizes a provider path to internal form: '\' to '/', trimmed.
fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_string()
}

/// A path written as PowerShell writes paths on this system, as a
/// FileSystem drive's are: each `\` and `/` as `\` on Windows and as `/`
/// elsewhere.
fn shown(path: &str) -> String {
    path.replace(['\\', '/'], std::path::MAIN_SEPARATOR_STR)
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

fn last(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// `name` inside the directory `parent`, both in internal form.
fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") }
}

/// The property `Invoke-Item` counts its calls in.
const INVOCATIONS: &str = "Invocations";

fn no_item(path: &str) -> PsError {
    PsError::new(ErrorCategory::ObjectNotFound, "NoItem", format!("{path} does not exist"))
}

fn exists(path: &str) -> PsError {
    PsError::new(ErrorCategory::ResourceExists, "ItemExists", format!("{path} already exists"))
}

fn into_itself(path: &str, target: &str) -> PsError {
    PsError::new(ErrorCategory::InvalidOperation, "IntoItself", format!("{target} is inside {path}, so {path} cannot be moved there"))
}

fn no_property(path: &str, name: &str) -> PsError {
    PsError::new(ErrorCategory::ObjectNotFound, "NoProperty", format!("{path} has no property {name}"))
}

fn is_directory(path: &str) -> PsError {
    PsError::new(ErrorCategory::InvalidOperation, "IsDirectory", format!("{path} is a directory"))
}

/// Whether two texts are the same without regard to case.
fn same_text(a: &str, b: &str) -> bool {
    a.chars().flat_map(char::to_lowercase).eq(b.chars().flat_map(char::to_lowercase))
}

/// Where the property `name` is in `properties`, matched without regard
/// to case, as PowerShell matches property names.
fn position(properties: &[(String, PsObject)], name: &str) -> Option<usize> {
    properties.iter().position(|(n, _)| same_text(n, name))
}

/// The property that names the user whose credential made an item.
const CREATED_BY: &str = "CreatedBy";

/// PowerShell's own wildcard pattern for `text`, matching without regard
/// to case, as `-Include` and `-Exclude` match.
fn wildcard(text: &str) -> PsResult<PsObject> {
    PsType::from_name("System.Management.Automation.WildcardPattern").new(&[text.into_ps()?, "IgnoreCase".into_ps()?])
}

/// Whether `name` matches a pattern [`wildcard`] made.
fn matches(pattern: &PsObject, name: &str) -> PsResult<bool> {
    bool::from_ps(&pattern.call("IsMatch", &[name.into_ps()?])?)
}

/// What the `-Filter` of the call running now selects. Text with `=` in
/// it selects each item whose property named by the text before the first
/// `=` has the text after it as its value, without regard to case. Other
/// text is a name pattern, matched by PowerShell's own wildcard rules
/// without regard to case. With no filter, every item is selected.
enum Selector {
    Every,
    Name(PsObject),
    Property { name: String, value: String },
}

impl Selector {
    fn of_call() -> PsResult<Selector> {
        let Some(text) = pwrs::provider::call().filter else {
            return Ok(Selector::Every);
        };
        match text.split_once('=') {
            Some(("", _)) => Err(PsError::new(ErrorCategory::InvalidArgument, "FilterWithoutName", format!("the filter {text} names no property before its ="))),
            Some((name, value)) => Ok(Selector::Property { name: name.to_string(), value: value.to_string() }),
            None => Ok(Selector::Name(wildcard(&text)?)),
        }
    }

    /// Whether the item `node`, named `name`, is selected.
    fn selects(&self, name: &str, node: &Node) -> PsResult<bool> {
        match self {
            Selector::Every => Ok(true),
            Selector::Name(pattern) => matches(pattern, name),
            Selector::Property { name: wanted, value } => match position(&node.properties, wanted) {
                Some(i) => Ok(same_text(&String::from_ps(&node.properties[i].1)?, value)),
                None => Ok(false),
            },
        }
    }
}

/// The parts of SDDL text in the order owner, group, access list and
/// audit list, each with its `O:`, `G:`, `D:` or `S:` prefix, or `None`
/// where the text has no such part. Each part starts at the letter
/// before a colon outside parentheses and runs to the next one.
fn sddl_parts(sddl: &str) -> [Option<&str>; 4] {
    let mut starts: Vec<(usize, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut previous: Option<(usize, char)> = None;
    for (i, c) in sddl.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                if let Some((at, letter)) = previous
                    && let Some(slot) = "OGDS".find(letter)
                {
                    starts.push((slot, at));
                }
            }
            _ => {}
        }
        previous = Some((i, c));
    }
    let mut parts = [None; 4];
    for (n, &(slot, at)) in starts.iter().enumerate() {
        let end = match starts.get(n + 1) {
            Some(&(_, next)) => next,
            None => sddl.len(),
        };
        parts[slot] = Some(&sddl[at..end]);
    }
    parts
}

/// A `Pwrs.MemItem` PSObject for a node on the drive named `drive`.
/// `FullName` is the path a user writes, drive included. The item has no
/// `Path` property: piped into a command, it would be passed as `-Path`,
/// in place of `PSPath` as `-LiteralPath`.
fn item_object(drive: &str, path: &str, node: &Node) -> PsResult<PsObject> {
    let obj = pwrs::object::new_psobject("Pwrs.MemItem");
    let name = if path.is_empty() { "".to_string() } else { last(path).to_string() };
    pwrs::object::add_note(&obj, "Name", name.into_ps()?)?;
    pwrs::object::add_note(&obj, "FullName", format!("{drive}:{}{}", std::path::MAIN_SEPARATOR, shown(path)).into_ps()?)?;
    match &node.kind {
        Kind::Dir => {
            pwrs::object::add_note(&obj, "IsContainer", true.into_ps()?)?;
            pwrs::object::add_note(&obj, "Length", PsObject::null())?;
        }
        Kind::File(content) => {
            pwrs::object::add_note(&obj, "IsContainer", false.into_ps()?)?;
            pwrs::object::add_note(&obj, "Length", (content.len() as i64).into_ps()?)?;
        }
    }
    Ok(obj)
}

fn to_item(drive: &str, path: &str, node: &Node) -> PsResult<Item> {
    let display = shown(path);
    let value = item_object(drive, path, node)?;
    Ok(match node.kind {
        Kind::Dir => Item::container(display, value),
        Kind::File(_) => Item::leaf(display, value),
    })
}

/// An in-memory filesystem; one tree per drive.
#[provider(name = "MemFs", capabilities = ["Filter", "Credentials"])]
pub struct MemFs {
    /// The name of this tree's drive. Every item's `FullName` starts
    /// with it.
    drive: String,
    /// The user name of the credential `New-PSDrive -Credential` gave the
    /// drive, which an item made without `-Credential` records as its
    /// maker.
    creator: Option<String>,
    /// Nodes by normalized path: segments joined by '/', the root is
    /// the empty string.
    tree: BTreeMap<String, Node>,
}

impl MemFs {
    fn empty(drive: &str, creator: Option<String>) -> MemFs {
        let mut tree = BTreeMap::new();
        tree.insert(String::new(), Node::new(Kind::Dir));
        MemFs { drive: drive.to_string(), creator, tree }
    }

    /// A new node of `kind`, whose `CreatedBy` property names the user of
    /// the call's credential, or of the drive's when the call has none. A
    /// node made with neither has no `CreatedBy`.
    fn made(&self, kind: Kind) -> PsResult<Node> {
        let mut node = Node::new(kind);
        let user = match pwrs::provider::call().credential {
            Some(credential) => Some(credential.user_name),
            None => self.creator.clone(),
        };
        if let Some(user) = user {
            node.properties.push((CREATED_BY.to_string(), user.into_ps()?));
        }
        Ok(node)
    }

    fn node(&self, path: &str) -> PsResult<&Node> {
        let p = norm(path);
        match self.tree.get(&p) {
            Some(node) => Ok(node),
            None => Err(no_item(&p)),
        }
    }

    fn node_mut(&mut self, path: &str) -> PsResult<&mut Node> {
        let p = norm(path);
        match self.tree.get_mut(&p) {
            Some(node) => Ok(node),
            None => Err(no_item(&p)),
        }
    }

    /// Refuses a path whose parent is not a directory of the tree.
    fn check_parent(&self, p: &str) -> PsResult<()> {
        let parent = parent_of(p);
        match self.tree.get(parent) {
            Some(Node { kind: Kind::Dir, .. }) => Ok(()),
            Some(_) => Err(PsError::new(ErrorCategory::InvalidOperation, "ParentIsFile", format!("the parent of {p} is a file"))),
            None => Err(PsError::new(ErrorCategory::ObjectNotFound, "NoParent", format!("parent of {p} does not exist"))),
        }
    }

    /// The value of the property `name` of the item at `path`.
    fn property(&self, path: &str, name: &str) -> PsResult<PsObject> {
        let node = self.node(path)?;
        match position(&node.properties, name) {
            Some(i) => Ok(node.properties[i].1.clone()),
            None => Err(no_property(&norm(path), name)),
        }
    }

    /// The item at `path` and, when `recurse` is set and it is a
    /// directory, everything under it: each node with its path below the
    /// item's, which is empty for the item itself.
    fn subtree(&self, path: &str, recurse: bool) -> PsResult<Vec<(String, Node)>> {
        let p = norm(path);
        let top = match self.tree.get(&p) {
            Some(node) => node.clone(),
            None => return Err(no_item(&p)),
        };
        let mut nodes = vec![(String::new(), top)];
        if recurse {
            let prefix = if p.is_empty() { String::new() } else { format!("{p}/") };
            for (k, node) in self.tree.iter() {
                if !k.is_empty() && *k != p && k.starts_with(&prefix) {
                    nodes.push((format!("/{}", &k[prefix.len()..]), node.clone()));
                }
            }
        }
        Ok(nodes)
    }

    /// Where a copy or a move of the item at `p` to `dest` lands, by the
    /// FileSystem provider's rule: inside `dest` when `dest` is a
    /// directory, and at `dest` otherwise. The root has no name to go
    /// inside with, so it lands at `dest`.
    fn landing(&self, p: &str, dest: &str) -> String {
        let target = norm(dest);
        match self.tree.get(&target) {
            Some(Node { kind: Kind::Dir, .. }) if !p.is_empty() => join(&target, last(p)),
            _ => target,
        }
    }

    /// Puts `nodes` from [`MemFs::subtree`] at `target` by the FileSystem
    /// provider's rules: a file replaces a file, and nothing else replaces
    /// what is already there. Returns the item at `target`.
    fn put_copy(&mut self, target: &str, nodes: Vec<(String, Node)>) -> PsResult<Option<Item>> {
        let file = matches!(nodes.first(), Some((_, Node { kind: Kind::File(_), .. })));
        match self.tree.get(target) {
            Some(Node { kind: Kind::File(_), .. }) if file => {}
            Some(Node { kind: Kind::Dir, .. }) if file => return Err(is_directory(target)),
            Some(_) => return Err(exists(target)),
            None => self.check_parent(target)?,
        }
        for (below, node) in nodes {
            self.tree.insert(format!("{target}{below}"), node);
        }
        self.item_at(target)
    }

    /// Moves the node at `p`, and every node below it, to `target`.
    fn relocate(&mut self, p: &str, target: &str) {
        let prefix = format!("{p}/");
        let moved: Vec<String> = self.tree.keys().filter(|k| k.as_str() == p || k.starts_with(&prefix)).cloned().collect();
        for old in moved {
            if let Some(node) = self.tree.remove(&old) {
                self.tree.insert(format!("{target}{}", &old[p.len()..]), node);
            }
        }
    }

    /// Makes room at `target` for a moved item, as on a FileSystem drive: a
    /// file moved onto a file replaces it under `-Force`, and anything else
    /// already there is refused.
    fn clear_for_move(&mut self, moving_file: bool, target: &str) -> PsResult<()> {
        match self.tree.get(target) {
            None => Ok(()),
            Some(Node { kind: Kind::File(_), .. }) if moving_file && pwrs::provider::call().force => {
                self.tree.remove(target);
                Ok(())
            }
            Some(_) => Err(exists(target)),
        }
    }

    /// Removes the node at `p` and every node below it.
    fn remove_subtree(&mut self, p: &str) {
        let prefix = format!("{p}/");
        self.tree.retain(|k, _| k.as_str() != p && !k.starts_with(&prefix));
    }

    /// The item at `p`, which an operation has just put there.
    fn item_at(&self, p: &str) -> PsResult<Option<Item>> {
        match self.tree.get(p) {
            Some(node) => Ok(Some(to_item(&self.drive, p, node)?)),
            None => Err(no_item(p)),
        }
    }

    /// Sets the property `name` of the item at `path`, adding it when
    /// the item does not have it, and returns it as it now is.
    fn put_property(&mut self, path: &str, name: &str, value: PsObject) -> PsResult<(String, PsObject)> {
        let node = self.node_mut(path)?;
        match position(&node.properties, name) {
            Some(i) => {
                node.properties[i].1 = value;
                Ok(node.properties[i].clone())
            }
            None => {
                node.properties.push((name.to_string(), value));
                Ok((name.to_string(), node.properties[node.properties.len() - 1].1.clone()))
            }
        }
    }
}

impl Provider for MemFs {
    fn default_drives() -> PsResult<Vec<(Drive, MemFs)>> {
        // Empty root: provider paths are drive-relative ("docs",
        // "docs/a.txt"), not qualified with the drive name.
        Ok(vec![(Drive { name: "mem".to_string(), root: String::new() }, MemFs::empty("mem", None))])
    }

    /// A fresh tree; the root the user gave is replaced by the empty
    /// root every drive of this provider uses. The drive records the user
    /// name of the credential `-Credential` gave.
    fn new_drive(name: &str, _root: &str) -> PsResult<(Drive, MemFs)> {
        let creator = pwrs::provider::call().credential.map(|credential| credential.user_name);
        Ok((Drive { name: name.to_string(), root: String::new() }, MemFs::empty(name, creator)))
    }

    fn is_valid_path(_path: &str) -> bool {
        true
    }

    fn item_exists(&mut self, path: &str) -> PsResult<bool> {
        Ok(self.tree.contains_key(&norm(path)))
    }

    fn is_item_container(&mut self, path: &str) -> PsResult<bool> {
        Ok(matches!(self.tree.get(&norm(path)), Some(Node { kind: Kind::Dir, .. })))
    }

    fn get_item(&mut self, path: &str) -> PsResult<Option<Item>> {
        let p = norm(path);
        match self.tree.get(&p) {
            Some(node) => Ok(Some(to_item(&self.drive, &p, node)?)),
            None => Ok(None),
        }
    }

    /// A file in memory has no program to open it, so `Invoke-Item` adds
    /// one to the file's `Invocations` property, which `Get-ItemProperty`
    /// reads. A directory refuses it.
    fn invoke_default_action(&mut self, path: &str) -> PsResult<()> {
        let p = norm(path);
        let node = self.node(path)?;
        if let Kind::Dir = node.kind {
            return Err(PsError::new(ErrorCategory::InvalidOperation, "IsDirectory", format!("{p} is a directory, which has no default action")));
        }
        let count = match position(&node.properties, INVOCATIONS) {
            Some(i) => i64::from_ps(&node.properties[i].1)?,
            None => 0,
        };
        let next = match count.checked_add(1) {
            Some(next) => next,
            None => {
                return Err(PsError::new(ErrorCategory::LimitsExceeded, "TooManyInvocations", format!("{p} has been invoked {count} times, the most its count can record")));
            }
        };
        self.put_property(path, INVOCATIONS, next.into_ps()?)?;
        Ok(())
    }

    /// The items in the directory at `path`, and with `recurse` every item
    /// below it, that the call's filter selects. A directory the filter
    /// does not select is still searched.
    fn get_child_items(&mut self, path: &str, recurse: bool) -> PsResult<Vec<Item>> {
        let base = norm(path);
        let selector = Selector::of_call()?;
        let mut out = Vec::new();
        for (p, node) in self.tree.iter() {
            if p.is_empty() || *p == base {
                continue;
            }
            let is_descendant = match base.is_empty() {
                true => true,
                false => p.starts_with(&format!("{base}/")),
            };
            if !is_descendant {
                continue;
            }
            let rel = if base.is_empty() { p.as_str() } else { &p[base.len() + 1..] };
            let direct = !rel.contains('/');
            if (recurse || direct) && selector.selects(last(p), node)? {
                out.push(to_item(&self.drive, p, node)?);
            }
        }
        Ok(out)
    }

    /// Makes a file or a directory. An item already at `path` is refused
    /// without `-Force`, as on a FileSystem drive. With `-Force`, a file
    /// replaces a file, a directory where a directory is returns that
    /// directory as it is, and a directory where a file is changes nothing
    /// and returns nothing. A file is never made where a directory is.
    fn new_item(&mut self, path: &str, item_type: &str, value: PsObject) -> PsResult<Option<Item>> {
        let p = norm(path);
        let is_dir = item_type.eq_ignore_ascii_case("directory") || item_type.eq_ignore_ascii_case("container");
        let force = pwrs::provider::call().force;
        match self.tree.get(&p).map(|node| matches!(node.kind, Kind::Dir)) {
            Some(true) if !is_dir => return Err(is_directory(&p)),
            Some(_) if !force => return Err(exists(&p)),
            Some(true) => return self.item_at(&p),
            Some(false) if is_dir => return Ok(None),
            Some(false) => {}
            None => self.check_parent(&p)?,
        }
        let kind = if is_dir {
            Kind::Dir
        } else {
            let content = if value.is_null() { String::new() } else { String::from_ps(&value)? };
            Kind::File(content)
        };
        let node = self.made(kind)?;
        let item = to_item(&self.drive, &p, &node)?;
        self.tree.insert(p, node);
        Ok(Some(item))
    }

    fn remove_item(&mut self, path: &str, recurse: bool) -> PsResult<()> {
        let p = norm(path);
        let children: Vec<String> = self.tree.keys().filter(|k| k.starts_with(&format!("{p}/"))).cloned().collect();
        if !children.is_empty() && !recurse {
            return Err(PsError::new(ErrorCategory::InvalidOperation, "HasChildren", format!("{p} has children; use -Recurse")));
        }
        for c in children {
            self.tree.remove(&c);
        }
        self.tree.remove(&p);
        Ok(())
    }

    /// Renames the item, and a directory's whole subtree with it; each
    /// node's properties and security descriptor go with it.
    fn rename_item(&mut self, path: &str, new_name: &str) -> PsResult<Option<Item>> {
        let p = norm(path);
        let parent = parent_of(&p).to_string();
        let target = if parent.is_empty() { norm(new_name) } else { format!("{parent}/{}", last(&norm(new_name))) };
        if !self.tree.contains_key(&p) {
            return Err(no_item(&p));
        }
        if target != p && self.tree.contains_key(&target) {
            return Err(exists(&target));
        }
        self.relocate(&p, &target);
        self.item_at(&target)
    }

    /// Copies an item with its properties and security descriptor, and
    /// with `recurse` a directory's whole subtree; without it, a directory
    /// is copied empty. As on a FileSystem drive, a copy onto a directory
    /// goes inside it, a file replaces a file of the same name, and
    /// anything else already there is refused.
    fn copy_item(&mut self, path: &str, dest: &str, recurse: bool) -> PsResult<Option<Item>> {
        let nodes = self.subtree(path, recurse)?;
        let target = self.landing(&norm(path), dest);
        self.put_copy(&target, nodes)
    }

    /// Copies as `copy_item` does, into another drive's tree.
    fn copy_item_to(&mut self, path: &str, dest_drive: &mut MemFs, dest: &str, recurse: bool) -> PsResult<Option<Item>> {
        let nodes = self.subtree(path, recurse)?;
        let target = dest_drive.landing(&norm(path), dest);
        dest_drive.put_copy(&target, nodes)
    }

    /// Moves an item, and a directory's whole subtree with it, properties
    /// and security descriptors included. As on a FileSystem drive, a move
    /// onto a directory goes inside it, a file moved onto a file replaces it
    /// under `-Force`, any other move onto an item already there or of a
    /// directory into itself is refused, and a move of an item onto itself
    /// changes nothing.
    fn move_item(&mut self, path: &str, dest: &str) -> PsResult<Option<Item>> {
        let p = norm(path);
        if p.is_empty() {
            return Err(PsError::new(ErrorCategory::InvalidOperation, "IsRoot", "a drive's root cannot be moved"));
        }
        let moving_file = match self.tree.get(&p) {
            Some(node) => matches!(node.kind, Kind::File(_)),
            None => return Err(no_item(&p)),
        };
        let target = self.landing(&p, dest);
        if target == p {
            return self.item_at(&p);
        }
        if target.starts_with(&format!("{p}/")) {
            return Err(into_itself(&p, &target));
        }
        self.clear_for_move(moving_file, &target)?;
        self.check_parent(&target)?;
        self.relocate(&p, &target);
        self.item_at(&target)
    }

    /// Moves as `move_item` does, into another drive's tree.
    fn move_item_to(&mut self, path: &str, dest_drive: &mut MemFs, dest: &str) -> PsResult<Option<Item>> {
        let p = norm(path);
        if p.is_empty() {
            return Err(PsError::new(ErrorCategory::InvalidOperation, "IsRoot", "a drive's root cannot be moved"));
        }
        let nodes = self.subtree(path, true)?;
        let target = dest_drive.landing(&p, dest);
        let moving_file = matches!(nodes.first(), Some((_, Node { kind: Kind::File(_), .. })));
        dest_drive.clear_for_move(moving_file, &target)?;
        dest_drive.check_parent(&target)?;
        for (below, node) in nodes {
            dest_drive.tree.insert(format!("{target}{below}"), node);
        }
        self.remove_subtree(&p);
        dest_drive.item_at(&target)
    }

    fn copy_property_to(&mut self, path: &str, name: &str, dest_drive: &mut MemFs, dest_path: &str, dest_name: &str) -> PsResult<Vec<(String, PsObject)>> {
        let value = self.property(path, name)?;
        Ok(vec![dest_drive.put_property(dest_path, dest_name, value)?])
    }

    fn move_property_to(&mut self, path: &str, name: &str, dest_drive: &mut MemFs, dest_path: &str, dest_name: &str) -> PsResult<Vec<(String, PsObject)>> {
        let value = self.property(path, name)?;
        dest_drive.node(dest_path)?;
        self.remove_property(path, name)?;
        Ok(vec![dest_drive.put_property(dest_path, dest_name, value)?])
    }

    /// A file's lines, or for a directory one `Pwrs.MemDir` object whose
    /// `Name` and `Entries` (its direct children) are note properties on
    /// the PSObject itself, which `$mem:path` reads through the content
    /// reader.
    fn get_content(&mut self, path: &str) -> PsResult<Vec<PsObject>> {
        let p = norm(path);
        match self.tree.get(&p).map(|node| &node.kind) {
            Some(Kind::File(content)) => content.lines().map(|l| l.to_string().into_ps()).collect(),
            Some(Kind::Dir) => {
                let prefix = if p.is_empty() { String::new() } else { format!("{p}/") };
                let entries = self.tree.keys().filter(|k| !k.is_empty() && k.starts_with(&prefix) && !k[prefix.len()..].contains('/')).count();
                let obj = pwrs::object::new_psobject("Pwrs.MemDir");
                pwrs::object::add_note(&obj, "Name", last(&p).to_string().into_ps()?)?;
                pwrs::object::add_note(&obj, "Entries", (entries as i64).into_ps()?)?;
                Ok(vec![obj])
            }
            None => Err(no_item(&p)),
        }
    }

    /// Adds the lines at the end of a file and changes nothing else about
    /// it. A file that is not there is made, with a `CreatedBy` as
    /// `New-Item` gives one.
    fn write_content(&mut self, path: &str, lines: Vec<PsObject>) -> PsResult<()> {
        let p = norm(path);
        let mut text = Vec::with_capacity(lines.len());
        for line in &lines {
            text.push(String::from_ps(line)?);
        }
        let added = text.join("\n");
        match self.tree.get_mut(&p) {
            Some(Node { kind: Kind::Dir, .. }) => Err(is_directory(&p)),
            Some(Node { kind: Kind::File(content), .. }) => {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(&added);
                Ok(())
            }
            None => {
                self.check_parent(&p)?;
                let node = self.made(Kind::File(added))?;
                self.tree.insert(p, node);
                Ok(())
            }
        }
    }

    /// Empties a file, which `Set-Content` does before it writes one that
    /// exists.
    fn clear_content(&mut self, path: &str) -> PsResult<()> {
        let p = norm(path);
        match self.tree.get_mut(&p) {
            Some(Node { kind: Kind::Dir, .. }) => Err(is_directory(&p)),
            Some(Node { kind: Kind::File(content), .. }) => {
                content.clear();
                Ok(())
            }
            None => Err(no_item(&p)),
        }
    }

    fn get_property(&mut self, path: &str, names: &[String]) -> PsResult<Vec<(String, PsObject)>> {
        let node = self.node(path)?;
        if names.is_empty() {
            return Ok(node.properties.clone());
        }
        names
            .iter()
            .map(|name| match position(&node.properties, name) {
                Some(i) => Ok(node.properties[i].clone()),
                None => Err(no_property(&norm(path), name)),
            })
            .collect()
    }

    fn set_property(&mut self, path: &str, values: &[(String, PsObject)]) -> PsResult<Vec<(String, PsObject)>> {
        values.iter().map(|(name, value)| self.put_property(path, name, value.clone())).collect()
    }

    /// Sets each value to `$null`, leaving the property in place.
    fn clear_property(&mut self, path: &str, names: &[String]) -> PsResult<Vec<(String, PsObject)>> {
        for name in names {
            self.property(path, name)?;
        }
        names.iter().map(|name| self.put_property(path, name, PsObject::null())).collect()
    }

    /// Stores the value as given, so `-PropertyType` is refused.
    fn new_property(&mut self, path: &str, name: &str, type_name: &str, value: PsObject) -> PsResult<Vec<(String, PsObject)>> {
        let force = pwrs::provider::call().force;
        if !type_name.is_empty() {
            return Err(PsError::new(
                ErrorCategory::InvalidArgument,
                "NoPropertyTypes",
                format!("memfs stores a property's value as given and has no property types, so -PropertyType {type_name} cannot apply"),
            ));
        }
        if !force && position(&self.node(path)?.properties, name).is_some() {
            return Err(PsError::new(
                ErrorCategory::ResourceExists,
                "PropertyExists",
                format!("{} already has a property {name}; -Force replaces it", norm(path)),
            ));
        }
        Ok(vec![self.put_property(path, name, value)?])
    }

    fn remove_property(&mut self, path: &str, name: &str) -> PsResult<()> {
        let p = norm(path);
        let node = self.node_mut(path)?;
        match position(&node.properties, name) {
            Some(i) => {
                node.properties.remove(i);
                Ok(())
            }
            None => Err(no_property(&p, name)),
        }
    }

    fn rename_property(&mut self, path: &str, name: &str, new_name: &str) -> PsResult<Vec<(String, PsObject)>> {
        let p = norm(path);
        let node = self.node_mut(path)?;
        let i = match position(&node.properties, name) {
            Some(i) => i,
            None => return Err(no_property(&p, name)),
        };
        if let Some(other) = position(&node.properties, new_name)
            && other != i
        {
            return Err(PsError::new(ErrorCategory::ResourceExists, "PropertyExists", format!("{p} already has a property {new_name}")));
        }
        node.properties[i].0 = new_name.to_string();
        Ok(vec![node.properties[i].clone()])
    }

    /// Replaces the destination property when the item has one.
    fn copy_property(&mut self, path: &str, name: &str, dest_path: &str, dest_name: &str) -> PsResult<Vec<(String, PsObject)>> {
        let value = self.property(path, name)?;
        Ok(vec![self.put_property(dest_path, dest_name, value)?])
    }

    fn move_property(&mut self, path: &str, name: &str, dest_path: &str, dest_name: &str) -> PsResult<Vec<(String, PsObject)>> {
        let value = self.property(path, name)?;
        self.node(dest_path)?;
        self.remove_property(path, name)?;
        Ok(vec![self.put_property(dest_path, dest_name, value)?])
    }

    /// The parts of the stored descriptor the call covers. A directory's
    /// is a `DirectorySecurity` and a file's a `FileSecurity`, so `Get-Acl`
    /// shows file rights by name.
    fn get_security_descriptor(&mut self, path: &str, sections: SecuritySections) -> PsResult<SecurityDescriptor> {
        let node = self.node(path)?;
        let [owner, group, access, audit] = sddl_parts(&node.sddl);
        let mut sddl = String::new();
        for (covered, part) in [(sections.owner, owner), (sections.group, group), (sections.access, access), (sections.audit, audit)] {
            if covered && let Some(part) = part {
                sddl.push_str(part);
            }
        }
        let kind = match node.kind {
            Kind::Dir => SecurityKind::Directory,
            Kind::File(_) => SecurityKind::File,
        };
        Ok(SecurityDescriptor { sddl, kind })
    }

    /// Replaces each part the SDDL text has, and leaves the parts it does
    /// not have as they were, such as the audit list of a descriptor read
    /// without `-Audit`.
    fn set_security_descriptor(&mut self, path: &str, sddl: &str) -> PsResult<()> {
        let node = self.node_mut(path)?;
        let merged: String = {
            let given = sddl_parts(sddl);
            let kept = sddl_parts(&node.sddl);
            given.iter().zip(kept.iter()).filter_map(|(new, old)| new.or(*old)).collect()
        };
        node.sddl = merged;
        Ok(())
    }

    fn make_path(parent: &str, child: &str) -> PsResult<String> {
        let parent = shown(parent.trim_end_matches(['\\', '/']));
        let child = shown(child.trim_start_matches(['\\', '/']));
        if parent.is_empty() {
            Ok(child)
        } else if child.is_empty() {
            Ok(parent)
        } else {
            Ok(format!("{parent}{}{child}", std::path::MAIN_SEPARATOR))
        }
    }

    fn get_parent_path(path: &str, _root: &str) -> PsResult<String> {
        Ok(shown(parent_of(&norm(path))))
    }

    fn get_child_name(path: &str) -> PsResult<String> {
        Ok(last(&norm(path)).to_string())
    }
}

/// The characters a note's name cannot contain: the separators, and the
/// characters PowerShell's wildcards and escapes use.
const NOT_IN_A_NAME: [char; 7] = ['\\', '/', '[', ']', '*', '?', '`'];

/// The call's `-Include` and `-Exclude` patterns, each made once.
struct Patterns {
    include: Vec<PsObject>,
    exclude: Vec<PsObject>,
}

impl Patterns {
    fn of_call() -> PsResult<Patterns> {
        let call = pwrs::provider::call();
        Ok(Patterns {
            include: call.include.iter().map(|pattern| wildcard(pattern)).collect::<PsResult<_>>()?,
            exclude: call.exclude.iter().map(|pattern| wildcard(pattern)).collect::<PsResult<_>>()?,
        })
    }

    /// Whether `name` matches one of the include patterns, when there are
    /// any, and none of the exclude patterns.
    fn admit(&self, name: &str) -> PsResult<bool> {
        let mut included = self.include.is_empty();
        for pattern in &self.include {
            if matches(pattern, name)? {
                included = true;
                break;
            }
        }
        if !included {
            return Ok(false);
        }
        for pattern in &self.exclude {
            if matches(pattern, name)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// The note name a path gives: the path without the separators at either
/// end. The empty name is the drive's root.
fn note_name(path: &str) -> &str {
    path.trim_matches(['\\', '/'])
}

/// A `Pwrs.MemNote` PSObject: the note's `Name`, its `Text` and its
/// `FullName`, the path a user writes, drive included.
fn note_object(drive: &str, name: &str, text: &str) -> PsResult<PsObject> {
    let obj = pwrs::object::new_psobject("Pwrs.MemNote");
    pwrs::object::add_note(&obj, "Name", name.into_ps()?)?;
    pwrs::object::add_note(&obj, "Text", text.into_ps()?)?;
    pwrs::object::add_note(&obj, "FullName", format!("{drive}:{}{name}", std::path::MAIN_SEPARATOR).into_ps()?)?;
    Ok(obj)
}

/// Notes in memory, all at the root of a drive: `notes:\shopping`,
/// `notes:\todo`, each a line of text. The provider declares
/// `ExpandWildcards`, so PowerShell passes a path with `*`, `?` or `[` to
/// `expand_path` and every other path on as it was typed, and the
/// provider itself matches names, applies `-Include` and `-Exclude`, and
/// reports a note that is not there. A note's name cannot contain `\`,
/// `/`, `[`, `]`, `*`, `?` or a backtick, so a name never needs escaping.
#[provider(name = "MemNotes", capabilities = ["ExpandWildcards"])]
pub struct MemNotes {
    /// The name of this list's drive.
    drive: String,
    /// Each note's text by name.
    notes: BTreeMap<String, String>,
}

impl MemNotes {
    fn empty(drive: &str) -> MemNotes {
        MemNotes { drive: drive.to_string(), notes: BTreeMap::new() }
    }

    fn missing(&self, name: &str) -> PsError {
        PsError::new(ErrorCategory::ObjectNotFound, "NoNote", format!("{}:{}{name} does not exist", self.drive, std::path::MAIN_SEPARATOR))
    }

    fn item(&self, name: &str, text: &str) -> PsResult<Item> {
        Ok(Item::leaf(name, note_object(&self.drive, name, text)?))
    }
}

impl Provider for MemNotes {
    fn default_drives() -> PsResult<Vec<(Drive, MemNotes)>> {
        Ok(vec![(Drive { name: "notes".to_string(), root: String::new() }, MemNotes::empty("notes"))])
    }

    fn new_drive(name: &str, _root: &str) -> PsResult<(Drive, MemNotes)> {
        Ok((Drive { name: name.to_string(), root: String::new() }, MemNotes::empty(name)))
    }

    /// The root is always there. A note is there when the list has it and
    /// its name passes the call's `-Include` and `-Exclude`, as a path on a
    /// FileSystem drive must.
    fn item_exists(&mut self, path: &str) -> PsResult<bool> {
        let name = note_name(path);
        if name.is_empty() {
            return Ok(true);
        }
        Ok(self.notes.contains_key(name) && Patterns::of_call()?.admit(name)?)
    }

    fn is_item_container(&mut self, path: &str) -> PsResult<bool> {
        Ok(note_name(path).is_empty())
    }

    /// The note at `path`, or the root. PowerShell does not check that a
    /// path without wildcards exists when a provider declares
    /// `ExpandWildcards`, so a note that is not there is an error here. A
    /// note whose name `-Include` or `-Exclude` leaves out gives nothing,
    /// as on a FileSystem drive.
    fn get_item(&mut self, path: &str) -> PsResult<Option<Item>> {
        let name = note_name(path);
        if name.is_empty() {
            let root = pwrs::object::new_psobject("Pwrs.MemNotes");
            pwrs::object::add_note(&root, "Name", self.drive.as_str().into_ps()?)?;
            pwrs::object::add_note(&root, "Count", (self.notes.len() as i64).into_ps()?)?;
            return Ok(Some(Item::container("", root)));
        }
        let Some(text) = self.notes.get(name) else {
            return Err(self.missing(name));
        };
        if !Patterns::of_call()?.admit(name)? {
            return Ok(None);
        }
        Ok(Some(self.item(name, text)?))
    }

    /// Every note, by name. Only the root has children.
    fn get_child_items(&mut self, path: &str, _recurse: bool) -> PsResult<Vec<Item>> {
        if !note_name(path).is_empty() {
            return Ok(Vec::new());
        }
        self.notes.iter().map(|(name, text)| self.item(name, text)).collect()
    }

    /// Adds a note whose text is `-Value`. A note already there is
    /// replaced only under `-Force`, and a folder cannot be made.
    fn new_item(&mut self, path: &str, item_type: &str, value: PsObject) -> PsResult<Option<Item>> {
        let name = note_name(path);
        if item_type.eq_ignore_ascii_case("directory") || item_type.eq_ignore_ascii_case("container") {
            return Err(PsError::new(ErrorCategory::InvalidArgument, "NoFolders", format!("{}: has notes only, and no folders", self.drive)));
        }
        if name.is_empty() || name.contains(NOT_IN_A_NAME) {
            return Err(PsError::new(
                ErrorCategory::InvalidArgument,
                "BadNoteName",
                format!("the note name '{name}' is empty or contains \\, /, [, ], *, ? or a backtick, which a note's name cannot"),
            ));
        }
        if self.notes.contains_key(name) && !pwrs::provider::call().force {
            return Err(PsError::new(ErrorCategory::ResourceExists, "NoteExists", format!("{}:{}{name} already exists; -Force replaces it", self.drive, std::path::MAIN_SEPARATOR)));
        }
        let text = String::from_ps(&value)?;
        let item = self.item(name, &text)?;
        self.notes.insert(name.to_string(), text);
        Ok(Some(item))
    }

    /// `Set-Item`: gives a note `-Value` as its text.
    fn set_item(&mut self, path: &str, value: PsObject) -> PsResult<Option<Item>> {
        let name = note_name(path);
        if !self.notes.contains_key(name) {
            return Err(self.missing(name));
        }
        let text = String::from_ps(&value)?;
        let item = self.item(name, &text)?;
        self.notes.insert(name.to_string(), text);
        Ok(Some(item))
    }

    fn remove_item(&mut self, path: &str, _recurse: bool) -> PsResult<()> {
        let name = note_name(path);
        if name.is_empty() {
            return Err(PsError::new(ErrorCategory::InvalidOperation, "IsRoot", "a drive's root cannot be removed"));
        }
        match self.notes.remove(name) {
            Some(_) => Ok(()),
            None => Err(self.missing(name)),
        }
    }

    fn get_content(&mut self, path: &str) -> PsResult<Vec<PsObject>> {
        let name = note_name(path);
        match self.notes.get(name) {
            Some(text) => Ok(vec![text.as_str().into_ps()?]),
            None => Err(self.missing(name)),
        }
    }

    /// The notes a path with wildcards names, matched by PowerShell's own
    /// wildcard rules without regard to case, each passing the call's
    /// `-Include` and `-Exclude`. A path below the root names none, since
    /// every note is at the root.
    fn expand_path(&mut self, path: &str) -> PsResult<Vec<String>> {
        let name = note_name(path);
        if name.contains(['\\', '/']) {
            return Ok(Vec::new());
        }
        let pattern = wildcard(name)?;
        let patterns = Patterns::of_call()?;
        let mut paths = Vec::new();
        for note in self.notes.keys() {
            if matches(&pattern, note)? && patterns.admit(note)? {
                paths.push(note.clone());
            }
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::sddl_parts;

    #[test]
    fn sddl_splits_into_its_four_parts_in_any_order() {
        assert_eq!(sddl_parts("O:BAG:SYD:(A;;FA;;;BA)S:(AU;FA;FW;;;WD)"), [Some("O:BA"), Some("G:SY"), Some("D:(A;;FA;;;BA)"), Some("S:(AU;FA;FW;;;WD)")]);
        assert_eq!(sddl_parts("D:PAI(A;OICI;FA;;;BA)O:S-1-5-32-544"), [Some("O:S-1-5-32-544"), None, Some("D:PAI(A;OICI;FA;;;BA)"), None]);
        assert_eq!(sddl_parts("G:DD"), [None, Some("G:DD"), None, None]);
        assert_eq!(sddl_parts(""), [None, None, None, None]);
    }

    #[test]
    fn a_colon_inside_an_entry_starts_no_part() {
        let conditional = "D:(XA;;FX;;;WD;(@User.Title==\"S:A\"))";
        assert_eq!(sddl_parts(conditional), [None, None, Some(conditional), None]);
    }
}

pwrs::export_module! {
    name: "MemFs",
    cmdlets: [],
    providers: [MemFs, MemNotes],
}
