//! An in-memory filesystem provider: `New-PSDrive -PSProvider MemFs`,
//! then `Get-ChildItem`, `New-Item`, `Set-Content`, `Get-Content`,
//! `Remove-Item`, `Rename-Item` against `mem:`. Every drive is its own
//! tree, held by the instance that serves it.

use pwrs::prelude::*;
use std::collections::BTreeMap;

#[derive(Clone)]
enum Node {
    Dir,
    File(String),
}

/// Normalizes a provider path to internal form: '\' to '/', trimmed.
fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_string()
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

/// A `Pwrs.MemItem` PSObject for a node.
fn item_object(path: &str, node: &Node) -> PsResult<PsObject> {
    let obj = pwrs::object::new_psobject("Pwrs.MemItem");
    let name = if path.is_empty() { "".to_string() } else { last(path).to_string() };
    pwrs::object::add_note(&obj, "Name", name.into_ps()?)?;
    pwrs::object::add_note(&obj, "Path", path.replace('/', "\\").into_ps()?)?;
    match node {
        Node::Dir => {
            pwrs::object::add_note(&obj, "IsContainer", true.into_ps()?)?;
            pwrs::object::add_note(&obj, "Length", PsObject::null())?;
        }
        Node::File(content) => {
            pwrs::object::add_note(&obj, "IsContainer", false.into_ps()?)?;
            pwrs::object::add_note(&obj, "Length", (content.len() as i64).into_ps()?)?;
        }
    }
    Ok(obj)
}

fn to_item(path: &str, node: &Node) -> PsResult<Item> {
    let display = path.replace('/', "\\");
    let value = item_object(path, node)?;
    Ok(match node {
        Node::Dir => Item::container(display, value),
        Node::File(_) => Item::leaf(display, value),
    })
}

/// An in-memory filesystem; one tree per drive.
#[provider(name = "MemFs")]
pub struct MemFs {
    /// Nodes by normalized path: segments joined by '/', the root is
    /// the empty string.
    tree: BTreeMap<String, Node>,
}

impl MemFs {
    fn empty() -> MemFs {
        let mut tree = BTreeMap::new();
        tree.insert(String::new(), Node::Dir);
        MemFs { tree }
    }
}

impl Provider for MemFs {
    fn default_drives() -> PsResult<Vec<(Drive, MemFs)>> {
        // Empty root: provider paths are drive-relative ("docs",
        // "docs/a.txt"), not qualified with the drive name.
        Ok(vec![(Drive { name: "mem".to_string(), root: String::new() }, MemFs::empty())])
    }

    /// A fresh tree; the root the user gave is replaced by the empty
    /// root every drive of this provider uses.
    fn new_drive(name: &str, _root: &str) -> PsResult<(Drive, MemFs)> {
        Ok((Drive { name: name.to_string(), root: String::new() }, MemFs::empty()))
    }

    fn is_valid_path(_path: &str) -> bool {
        true
    }

    fn item_exists(&mut self, path: &str) -> PsResult<bool> {
        Ok(self.tree.contains_key(&norm(path)))
    }

    fn is_item_container(&mut self, path: &str) -> PsResult<bool> {
        Ok(matches!(self.tree.get(&norm(path)), Some(Node::Dir)))
    }

    fn get_item(&mut self, path: &str) -> PsResult<Option<Item>> {
        let p = norm(path);
        match self.tree.get(&p) {
            Some(node) => Ok(Some(to_item(&p, node)?)),
            None => Ok(None),
        }
    }

    fn get_child_items(&mut self, path: &str, recurse: bool) -> PsResult<Vec<Item>> {
        let base = norm(path);
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
            if recurse || direct {
                out.push(to_item(p, node)?);
            }
        }
        Ok(out)
    }

    fn new_item(&mut self, path: &str, item_type: &str, value: PsObject) -> PsResult<Option<Item>> {
        let p = norm(path);
        let parent = parent_of(&p).to_string();
        if !parent.is_empty() && !self.tree.contains_key(&parent) {
            return Err(PsError::new(ErrorCategory::ObjectNotFound, "NoParent", format!("parent of {p} does not exist")));
        }
        let is_dir = item_type.eq_ignore_ascii_case("directory") || item_type.eq_ignore_ascii_case("container");
        let node = if is_dir {
            Node::Dir
        } else {
            let content = if value.is_null() { String::new() } else { String::from_ps(&value)? };
            Node::File(content)
        };
        self.tree.insert(p.clone(), node.clone());
        Ok(Some(to_item(&p, &node)?))
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

    fn rename_item(&mut self, path: &str, new_name: &str) -> PsResult<Option<Item>> {
        let p = norm(path);
        let parent = parent_of(&p).to_string();
        let target = if parent.is_empty() { norm(new_name) } else { format!("{parent}/{}", last(&norm(new_name))) };
        let node = match self.tree.remove(&p) {
            Some(n) => n,
            None => return Err(PsError::new(ErrorCategory::ObjectNotFound, "NoItem", format!("{p} does not exist"))),
        };
        self.tree.insert(target.clone(), node.clone());
        Ok(Some(to_item(&target, &node)?))
    }

    /// A file's lines, or for a directory one `Pwrs.MemDir` object whose
    /// `Name` and `Entries` (its direct children) are note properties on
    /// the PSObject itself, which `$mem:path` reads through the content
    /// reader.
    fn get_content(&mut self, path: &str) -> PsResult<Vec<PsObject>> {
        let p = norm(path);
        match self.tree.get(&p) {
            Some(Node::File(content)) => content.lines().map(|l| l.to_string().into_ps()).collect(),
            Some(Node::Dir) => {
                let prefix = if p.is_empty() { String::new() } else { format!("{p}/") };
                let entries = self.tree.keys().filter(|k| !k.is_empty() && k.starts_with(&prefix) && !k[prefix.len()..].contains('/')).count();
                let obj = pwrs::object::new_psobject("Pwrs.MemDir");
                pwrs::object::add_note(&obj, "Name", last(&p).to_string().into_ps()?)?;
                pwrs::object::add_note(&obj, "Entries", (entries as i64).into_ps()?)?;
                Ok(vec![obj])
            }
            None => Err(PsError::new(ErrorCategory::ObjectNotFound, "NoItem", format!("{p} does not exist"))),
        }
    }

    fn set_content(&mut self, path: &str, content: Vec<PsObject>) -> PsResult<()> {
        let p = norm(path);
        let mut lines = Vec::with_capacity(content.len());
        for line in &content {
            lines.push(String::from_ps(line)?);
        }
        self.tree.insert(p, Node::File(lines.join("\n")));
        Ok(())
    }

    fn make_path(parent: &str, child: &str) -> PsResult<String> {
        let parent = parent.trim_end_matches(['\\', '/']);
        let child = child.trim_start_matches(['\\', '/']);
        if parent.is_empty() {
            Ok(child.to_string())
        } else if child.is_empty() {
            Ok(parent.to_string())
        } else {
            Ok(format!("{parent}\\{child}"))
        }
    }

    fn get_parent_path(path: &str, _root: &str) -> PsResult<String> {
        Ok(parent_of(&norm(path)).replace('/', "\\"))
    }

    fn get_child_name(path: &str) -> PsResult<String> {
        Ok(last(&norm(path)).to_string())
    }
}

pwrs::export_module! {
    name: "MemFs",
    cmdlets: [],
    providers: [MemFs],
}
