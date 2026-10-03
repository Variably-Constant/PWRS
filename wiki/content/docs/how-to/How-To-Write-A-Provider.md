---
title: How To Write A Provider
weight: 3
---

A PowerShell provider from a Rust trait. Source: `crates/pwrs/src/provider.rs` (the `Provider` trait, op codes and dispatch), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/ProviderBase.cs` (the `NavigationCmdletProvider` that forwards to Rust), `crates/pwrs-macros/src/completers.rs` (`#[provider]`), and the worked example `examples/memfs`.

## The trait

```rust
use pwrs::prelude::*;

/// An in-memory filesystem; one tree per drive.
#[provider(name = "MemFs")]
pub struct MemFs {
    tree: BTreeMap<String, Node>,
}

impl Provider for MemFs {
    fn default_drives() -> PsResult<Vec<(Drive, MemFs)>> {
        Ok(vec![(Drive { name: "mem".to_string(), root: String::new() }, MemFs::empty())])
    }
    fn new_drive(name: &str, _root: &str) -> PsResult<(Drive, MemFs)> {
        Ok((Drive { name: name.to_string(), root: String::new() }, MemFs::empty()))
    }
    fn item_exists(&mut self, path: &str) -> PsResult<bool> { /* ... */ }
    fn is_item_container(&mut self, path: &str) -> PsResult<bool> { /* ... */ }
    fn get_item(&mut self, path: &str) -> PsResult<Option<Item>> { /* ... */ }
    fn get_child_items(&mut self, path: &str, recurse: bool) -> PsResult<Vec<Item>> { /* ... */ }
    fn new_item(&mut self, path: &str, item_type: &str, value: PsObject) -> PsResult<Option<Item>> { /* ... */ }
    fn remove_item(&mut self, path: &str, recurse: bool) -> PsResult<()> { /* ... */ }
    fn rename_item(&mut self, path: &str, new_name: &str) -> PsResult<Option<Item>> { /* ... */ }
    fn get_content(&mut self, path: &str) -> PsResult<Vec<PsObject>> { /* ... */ }
    fn set_content(&mut self, path: &str, content: Vec<PsObject>) -> PsResult<()> { /* ... */ }
    fn make_path(parent: &str, child: &str) -> PsResult<String> { /* ... */ }
    fn get_parent_path(path: &str, _root: &str) -> PsResult<String> { /* ... */ }
    fn get_child_name(path: &str) -> PsResult<String> { /* ... */ }
}

pwrs::export_module! {
    name: "MemFs",
    cmdlets: [],
    providers: [MemFs],
}
```

One value of the type serves one drive. `default_drives` makes the instances for the drives that exist at import; `new_drive` makes one for each `New-PSDrive`; every item, container and content operation on that drive takes it as `&mut self`; `Remove-PSDrive` calls `remove_drive` and drops it. The path methods and `is_valid_path` are associated functions, because the engine asks them before it has resolved a drive.

Every method has a default. `default_drives` defaults to none and `new_drive` to an error, so a provider chooses which it offers; `remove_drive` does nothing; `is_valid_path` defaults to true; `get_child_names` and `has_child_items` derive from `get_child_items`; `make_path`, `get_parent_path` and `get_child_name` join and split on `\` and `/`, and `normalize_relative_path` hands the path back unchanged; everything else returns a `NotImplemented` error naming the operation, so a `Set-Content` against a provider that did not implement `set_content` fails with `PwrsProviderUnsupported`.

The full method list, grouped as in the source:

| Group | Methods |
|---|---|
| drives | `default_drives()`, `new_drive(name, root)`, `remove_drive(&mut self)` |
| paths, associated functions | `is_valid_path`, `make_path(parent, child)`, `get_parent_path(path, root)`, `get_child_name`, `normalize_relative_path(path, base)` |
| items | `item_exists`, `is_item_container`, `get_item`, `set_item(path, value)`, `clear_item` |
| containers | `get_child_items(path, recurse)`, `get_child_names`, `has_child_items`, `new_item(path, item_type, value)`, `remove_item(path, recurse)`, `rename_item(path, new_name)`, `copy_item(path, dest, recurse)` |
| content | `get_content`, `set_content(path, content)`, `clear_content` |

## Items and drives

`Item { path, value, is_container }` is what a provider yields: the provider path, the object PowerShell shows for it, and whether it is a container. `Item::leaf(path, value)` and `Item::container(path, value)` build them. The managed side writes each with `WriteItemObject(value, path, isContainer)`.

The value can be any object. memfs builds a `PSObject` with `PSTypeName` `Pwrs.MemItem` and `Name`, `Path`, `IsContainer` and `Length` note properties through `pwrs::object::new_psobject` and `add_note`; a copied class works as well.

`Drive { name, root }` describes a drive. `default_drives` returns the ones created at import, each paired with the instance that serves it; `new_drive` returns the pair for a `New-PSDrive`. The root a provider returns is what the engine registers, which is how memfs replaces the root the user typed with the empty root its paths assume.

## Paths

The engine passes provider paths as the user typed them relative to the drive, with `\` on Windows. memfs declares its default drive with an empty root so paths arrive drive-relative (`docs`, `docs\a.txt`), normalizes `\` to `/`, and stores nodes under those keys. `make_path`, `get_parent_path` and `get_child_name` must agree with that convention, which is why memfs implements all three instead of taking the defaults.

## State

State that belongs to a drive lives on the instance: memfs keeps its tree there, so `New-PSDrive -Name mem2 -PSProvider MemFs -Root x` is a second, empty filesystem and `Remove-PSDrive mem2` frees it. The engine creates its drives per runspace, so two runspaces never share an instance, and the managed drive object serializes the operations on one drive, so a `&mut self` method never runs concurrently with another on the same instance. A drive that is never removed is dropped when the engine's drive object is collected. State shared across drives goes in a `static`.

## What the engine does for you

`ProviderBase` implements `NavigationCmdletProvider` and `IContentCmdletProvider`: it registers with `ProviderCapabilities.ShouldProcess`, calls `ShouldProcess` before `remove_item` and `rename_item` (so `-WhatIf` and `-Confirm` work), buffers `Set-Content` input and hands it to `set_content` when the writer closes, and turns `get_content` output into an `IContentReader` that hands each result over as the module returned it, the `PSObject` a wrapped one comes in included, so note properties on the wrapper reach `$drive:path` (the memfs example answers a directory as one `Pwrs.MemDir` object whose `Name` and `Entries` are such properties). Each drive is a `PwrsDriveInfo`, a `PSDriveInfo` carrying the instance pointer; every operation is forwarded with the pointer of the drive the engine resolved for it, and an operation the engine issues without a drive of this provider (a provider-qualified path such as `MemFs::x`) fails with `PwrsProviderNoDrive`. `capabilities = ["Filter", "Include"]` on `#[provider]` adds `ProviderCapabilities` flags by name.

## Using it

```powershell
Import-Module ./target/pwrs/MemFs/MemFs.psd1
Get-PSProvider MemFs
New-Item -Path 'mem:\docs' -ItemType Directory
Set-Content -Path 'mem:\docs\a.txt' -Value 'hello'
Get-ChildItem -Path 'mem:\docs' -Recurse
Get-Content -Path 'mem:\docs\a.txt'
Rename-Item -Path 'mem:\docs\a.txt' -NewName 'b.txt'
Remove-Item -Path 'mem:\docs' -Recurse
New-PSDrive -Name mem2 -PSProvider MemFs -Root x    # a second, empty tree
Remove-PSDrive -Name mem2                           # frees it
```

`examples/memfs/tests/MemFs.Tests.ps1` covers each of those in both hosts. A module with only a provider exports no cmdlets; `Import-Module -Assembly` registers the provider from the shell assembly's `[CmdletProvider]` attribute.
