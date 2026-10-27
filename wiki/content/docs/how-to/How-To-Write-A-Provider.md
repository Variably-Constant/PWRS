---
title: How To Write A Provider
weight: 3
---

A PowerShell provider from a Rust trait. Source: `crates/pwrs/src/provider.rs` (the `Provider` trait, op codes and dispatch), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/ProviderBase.cs` (the `NavigationCmdletProvider` that forwards to Rust) and `ItemSecurity.cs` beside it, `crates/pwrs-macros/src/completers.rs` (`#[provider]`), and the worked example `examples/memfs`.

## The trait

```rust
use pwrs::prelude::*;

/// An in-memory filesystem; one tree per drive.
#[provider(name = "MemFs", capabilities = ["Filter", "Credentials"])]
pub struct MemFs {
    drive: String,
    creator: Option<String>,
    tree: BTreeMap<String, Node>,
}

impl Provider for MemFs {
    fn default_drives() -> PsResult<Vec<(Drive, MemFs)>> {
        Ok(vec![(Drive { name: "mem".to_string(), root: String::new() }, MemFs::empty("mem", None))])
    }
    fn new_drive(name: &str, _root: &str) -> PsResult<(Drive, MemFs)> {
        let creator = pwrs::provider::call().credential.map(|credential| credential.user_name);
        Ok((Drive { name: name.to_string(), root: String::new() }, MemFs::empty(name, creator)))
    }
    fn item_exists(&mut self, path: &str) -> PsResult<bool> { /* ... */ }
    fn is_item_container(&mut self, path: &str) -> PsResult<bool> { /* ... */ }
    fn get_item(&mut self, path: &str) -> PsResult<Option<Item>> { /* ... */ }
    fn get_child_items(&mut self, path: &str, recurse: bool) -> PsResult<Vec<Item>> { /* ... */ }
    fn new_item(&mut self, path: &str, item_type: &str, value: PsObject) -> PsResult<Option<Item>> { /* ... */ }
    fn remove_item(&mut self, path: &str, recurse: bool) -> PsResult<()> { /* ... */ }
    fn rename_item(&mut self, path: &str, new_name: &str) -> PsResult<Option<Item>> { /* ... */ }
    fn copy_item(&mut self, path: &str, dest: &str, recurse: bool) -> PsResult<Option<Item>> { /* ... */ }
    fn move_item(&mut self, path: &str, dest: &str) -> PsResult<Option<Item>> { /* ... */ }
    // ... and copy_item_to and move_item_to, to another drive
    fn invoke_default_action(&mut self, path: &str) -> PsResult<()> { /* ... */ }
    fn get_content(&mut self, path: &str) -> PsResult<Vec<PsObject>> { /* ... */ }
    fn write_content(&mut self, path: &str, lines: Vec<PsObject>) -> PsResult<()> { /* ... */ }
    fn clear_content(&mut self, path: &str) -> PsResult<()> { /* ... */ }
    fn get_property(&mut self, path: &str, names: &[String]) -> PsResult<Vec<(String, PsObject)>> { /* ... */ }
    fn set_property(&mut self, path: &str, values: &[(String, PsObject)]) -> PsResult<Vec<(String, PsObject)>> { /* ... */ }
    // ... and clear_property, new_property, remove_property, rename_property,
    // copy_property and move_property
    fn get_security_descriptor(&mut self, path: &str, sections: SecuritySections) -> PsResult<SecurityDescriptor> { /* ... */ }
    fn set_security_descriptor(&mut self, path: &str, sddl: &str) -> PsResult<()> { /* ... */ }
    fn make_path(parent: &str, child: &str) -> PsResult<String> { /* ... */ }
    fn get_parent_path(path: &str, _root: &str) -> PsResult<String> { /* ... */ }
    fn get_child_name(path: &str) -> PsResult<String> { /* ... */ }
}

pwrs::export_module! {
    name: "MemFs",
    cmdlets: [],
    providers: [MemFs, MemNotes],
}
```

The module has a second provider, MemNotes, which [resolves wildcards itself](#expanding-wildcards-in-the-provider).

Each drive has its own value of the type. `default_drives` makes the instances for the drives that exist at import; `new_drive` makes one for each `New-PSDrive`; every item, container, content, property and security descriptor operation on that drive takes it as `&mut self`; `Remove-PSDrive` calls `remove_drive` and drops it. The path methods and `is_valid_path` are associated functions, because PowerShell calls them before it has found which drive a path is on.

Every method has a default:

- `default_drives` returns no drives and `new_drive` returns an error, so a provider implements the one it offers;
- `remove_drive` does nothing;
- `is_valid_path` returns true;
- `get_child_names` and `has_child_items` are built on `get_child_items`;
- `expand_path` returns the path as given;
- `make_path`, `get_parent_path` and `get_child_name` join and split on `\` and `/`, and `normalize_relative_path` returns `None`, so PowerShell works out a relative path itself from those three;
- everything else returns a `NotImplemented` error naming the operation, so a `Set-Content` on a provider that did not implement `write_content` fails with the id `PwrsProviderUnsupported` and the message `this provider does not implement write_content`.

The full method list, grouped as in the source:

| Group | Methods |
|---|---|
| drives | `default_drives()`, `new_drive(name, root)`, `remove_drive(&mut self)` |
| paths, associated functions | `is_valid_path`, `make_path(parent, child)`, `get_parent_path(path, root)`, `get_child_name`, `normalize_relative_path(path, base)` |
| items | `item_exists`, `is_item_container`, `get_item`, `set_item(path, value)`, `clear_item`, `invoke_default_action`, `expand_path` |
| containers | `get_child_items(path, recurse)`, `get_child_names`, `has_child_items`, `new_item(path, item_type, value)`, `remove_item(path, recurse)`, `rename_item(path, new_name)`, `copy_item(path, dest, recurse)`, `copy_item_to(path, dest_drive, dest, recurse)`, `move_item(path, dest)`, `move_item_to(path, dest_drive, dest)` |
| content | `get_content`, `write_content(path, lines)`, `clear_content` |
| item properties | `get_property(path, names)`, `set_property(path, values)`, `clear_property(path, names)`, `new_property(path, name, type_name, value)`, `remove_property(path, name)`, `rename_property(path, name, new_name)`, `copy_property(path, name, dest_path, dest_name)`, `move_property(path, name, dest_path, dest_name)`, `copy_property_to(path, name, dest_drive, dest_path, dest_name)`, `move_property_to(path, name, dest_drive, dest_path, dest_name)` |
| security descriptors | `get_security_descriptor(path, sections)`, `set_security_descriptor(path, sddl)` |

## Items and drives

`Item { path, value, is_container }` is what a provider yields: the provider path, the object PowerShell shows for it, and whether it is a container. `Item::leaf(path, value)` and `Item::container(path, value)` build them. The managed side writes each with `WriteItemObject(value, path, isContainer)`.

The value can be any object, a copied class among them. memfs builds a `PSObject` with `pwrs::object::new_psobject` and `add_note`. Its `PSTypeName` is `Pwrs.MemItem`, and its note properties are `Name`, `FullName`, `IsContainer` and `Length`. `FullName` is the item's path with its drive, written with the separator PowerShell uses on the system: `mem:\docs\a.txt` on Windows, and `mem:/docs/a.txt` on Linux, macOS and FreeBSD. Each memfs drive stores its own name to build it.

Give an item no property named `Path`. When an item is piped into a command such as `Get-Content`, PowerShell passes the item's `Path` property as `-Path`, if it has one. `-Path` reads `[` and `]` in a name as wildcards, so an item named `a[1].txt` is not found. Without a `Path` property, PowerShell passes the item's `PSPath` as `-LiteralPath`, which reads the name as written. Files on a FileSystem drive work that way.

`Drive { name, root }` describes a drive. `default_drives` returns the drives that exist at import, each with its instance. `new_drive` returns one drive and its instance for each `New-PSDrive`. The root a provider returns is where its paths start. memfs returns an empty root, whatever root the user typed. `ProviderBase` puts the drive's name, a colon and a separator in front of that root, so `Get-PSDrive mem` shows `mem:\` on Windows and `mem:/` on Linux, macOS and FreeBSD. The separator is the one PowerShell writes paths with on the system. PowerShell takes a path as a drive's root only when it ends in that separator. With the other one, commands on the root fail.

## Paths

Every provider path on a drive starts with the drive's prefix: `mem:\` for the `mem` drive on Windows, and `mem:/` on Linux, macOS and FreeBSD. `ProviderBase` takes the prefix off before a path reaches Rust, and adds it back to the paths Rust returns. So a provider gets paths that start at the root it gave. memfs has an empty root, so it gets paths such as `docs` and `docs\a.txt`. It changes each `\` to `/` and stores each item under that path, so a path typed with either separator names the same item. memfs implements `make_path`, `get_parent_path` and `get_child_name` itself. The paths they return use the system's separator, `std::path::MAIN_SEPARATOR`, as a FileSystem drive's paths do.

Sometimes PowerShell takes off only the drive's name and colon before it calls `get_parent_path` or `make_path`. For `Split-Path mem:\docs\a.txt -Parent`, it passes `\docs\a.txt`, and it adds `mem:` to the result itself. `ProviderBase` takes off the leading `\` too, so Rust gets `docs\a.txt` as usual. It puts the system's separator in front of the path Rust returns. The results match a FileSystem drive in pwsh 7.6.7 and Windows PowerShell 5.1 on Windows, pwsh 7.6.6 on Linux and pwsh 7.5.5 on FreeBSD:

- `Split-Path -Parent` returns `mem:\docs` for `mem:\docs\a.txt`, `mem:\` for `mem:\docs`, and an empty string for `mem:\`. On Linux and FreeBSD the first two are `mem:/docs` and `mem:/`.
- With the location at `mem:\docs`, `Resolve-Path mem:\docs\sub\deep.txt -Relative` returns `.\sub\deep.txt` on Windows and `./sub/deep.txt` on Linux and FreeBSD.
- A path given with the other separator comes back with the system's. On Windows, `Split-Path mem:/docs/sub/a.txt -Parent` returns `mem:\docs\sub`, as `Split-Path C:/Windows/System32/drivers -Parent` returns `C:\Windows\System32`.

An item's `PSPath` names its drive too, such as `MemFs::mem:\docs\a.txt`. For a piped item, PowerShell passes that path with no drive set. `ProviderBase` then sends the path to the drive whose prefix starts it. So `Get-ChildItem mem:\docs | Get-ItemProperty` and the other piped commands reach the right drive.

## State

State that belongs to a drive lives on the instance. memfs stores its tree there, so `New-PSDrive -Name mem2 -PSProvider MemFs -Root x` is a second, empty filesystem, and `Remove-PSDrive mem2` frees it. PowerShell makes each runspace's drives for that runspace, so two runspaces never share an instance. Operations on one drive run one at a time, so a `&mut self` method never runs while another runs on the same instance. A drive that is never removed is dropped when .NET frees PowerShell's object for the drive. State shared across drives goes in a `static`.

## The call's filter, patterns, -Force and credential

`pwrs::provider::call()` returns the `Call` of the operation running on the thread. Its fields are the values PowerShell's own providers read from `this.Filter`, `this.Include`, `this.Exclude`, `this.Force` and `this.Credential`:

| Field | Type | What it is |
|---|---|---|
| `filter` | `Option<String>` | the text of `-Filter` |
| `include` | `Vec<String>` | the `-Include` patterns |
| `exclude` | `Vec<String>` | the `-Exclude` patterns |
| `force` | `bool` | whether `-Force` was given |
| `credential` | `Option<PsCredential>` | the `-Credential` given |

Any provider method can read it, and `Call` is in the prelude:

```rust
fn new_property(&mut self, path: &str, name: &str, type_name: &str, value: PsObject) -> PsResult<Vec<(String, PsObject)>> {
    let force = pwrs::provider::call().force;
    // ...
}
```

An operation that runs inside another has its own `Call`, and the outer one's comes back when it returns. Outside an operation, and on a thread the operation did not start on, every field is empty.

`capabilities = [...]` on `#[provider]` adds `ProviderCapabilities` flags by name. Measured in pwsh 7.6.7 and Windows PowerShell 5.1, and read in PowerShell's source, each one changes this:

| Capability | Without it | With it |
|---|---|---|
| `Filter` | `-Filter` is refused: `The provider does not support the use of filters` | the text reaches `filter`, and PowerShell never reads it, so what it selects is up to the provider |
| `Credentials` | `-Credential` is refused | the credential reaches `credential` |
| `Include`, `Exclude` | PowerShell applies `-Include` and `-Exclude` | nothing changes: PowerShell reads neither flag, and applies both itself |
| `ExpandWildcards` | PowerShell resolves a path with wildcards through `get_child_names` | PowerShell passes such a path to `expand_path`, and every other path as it was typed; see [Expanding wildcards in the provider](#expanding-wildcards-in-the-provider) |

`include` and `exclude` are filled whatever the provider declares, so a provider can read them, such as to skip work.

A provider that declares `Filter` returns the items its filter selects from `get_child_items` and `get_child_names`, and with `recurse` it still searches a folder the filter leaves out. `ProviderBase` gives PowerShell what a FileSystem drive gives for the rest:

- PowerShell calls `HasChildItems` with the filter set before it lists a level of a wildcard path, and leaves the level out when it returns false. `ProviderBase` runs `has_child_items` without the filter or the patterns, as a FileSystem drive does.
- When PowerShell walks into every folder whatever the filter, it calls `GetChildNames` with `ReturnAllContainers`. It does that for `-Recurse` with `-Include` or `-Name`, and for each level of a wildcard path before the last. `ProviderBase` then follows the names `get_child_names` gives with each folder they leave out, from a `get_child_items` without the filter. The `-Depth` walk finds its folders the same way.

PowerShell passes the filter to `get_item` and the other item methods too. A FileSystem drive ignores it there, so `Get-Item C:\a.txt -Filter *.log` returns the file, and memfs does the same.

memfs's filter is a name pattern or a property test:

- Text with `=` in it selects the items whose property, named before the first `=`, has the value after it, without regard to case. `Get-ChildItem mem:\docs -Recurse -Filter Color=red` finds every item whose `Color` is `red` or `Red`. `-Filter =red` names no property, and is refused.
- Other text is a name pattern, matched by PowerShell's own wildcard rules without regard to case. memfs makes a `WildcardPattern` with `PsType::new(&[text, "IgnoreCase"])` and calls its `IsMatch`.

With FileSystem's two capabilities, `Filter` and `Credentials`, memfs gives the items a FileSystem drive gives for every `-Filter`, `-Include`, `-Exclude`, wildcard and escaped name measured in pwsh 7.6.7 and Windows PowerShell 5.1. Only the order differs: a FileSystem drive lists folders first, and memfs lists by name.

`New-PSDrive -Credential` reaches `new_drive` as `credential`, and later calls on that drive have no credential. So a provider that needs a drive's credential records it on the drive's instance. memfs records the user name, and gives each item it makes a `CreatedBy` property. It names the user of `-Credential`, or of the drive's credential when the call has none:

```powershell
$cred = Get-Credential ada
New-Item -Path 'mem:\docs\a.txt' -ItemType File -Credential $cred
(Get-ItemProperty -Path 'mem:\docs\a.txt').CreatedBy    # ada
New-PSDrive -Name work -PSProvider MemFs -Root x -Credential $cred
Set-Content -Path 'work:\b.txt' -Value 'b'
(Get-ItemProperty -Path 'work:\b.txt').CreatedBy        # ada
```

An item made with neither has no `CreatedBy`, and a copy has every property of what it copies, `CreatedBy` among them. memfs checks no password.

## Expanding wildcards in the provider

A provider that declares `ExpandWildcards` resolves wildcards itself. PowerShell passes a path with `*`, `?` or `[` to `expand_path`, which returns the paths it names, and runs the command once for each. Every other path reaches the provider's methods as it was typed. So the provider does what PowerShell does for the others:

- it matches each level of the path by PowerShell's wildcard rules;
- it applies `-Include` and `-Exclude`, both to the paths `expand_path` returns and to a path without wildcards;
- it reports a path that is not there, which `get_item` has to do itself;
- it removes backtick escapes: a path typed as ``a`[1`]`` arrives with its backticks.

Two cases cannot give a FileSystem drive's results even then. `Get-ChildItem mem:\docs -Include *.txt` lists the folder's `.txt` files, where a FileSystem drive lists nothing, because PowerShell calls `expand_path` only for a path with wildcards. And a name with `[`, `]`, `*`, `?` or a backtick in it cannot be found every time. `Get-Item` passes on the paths `expand_path` returns as they are, while `Get-Content` escapes them before they reach the provider.

The memfs example's second provider, MemNotes, declares `ExpandWildcards` alone. Its `notes:` drive has notes at its root, each a line of text. A note's name cannot contain `\`, `/`, `[`, `]`, `*`, `?` or a backtick, so no name ever needs escaping:

```rust
fn expand_path(&mut self, path: &str) -> PsResult<Vec<String>> {
    let name = note_name(path);
    if name.contains(['\\', '/']) {
        return Ok(Vec::new());
    }
    let pattern = wildcard(name)?;          // a WildcardPattern that ignores case
    let patterns = Patterns::of_call()?;    // the call's -Include and -Exclude
    let mut paths = Vec::new();
    for note in self.notes.keys() {
        if matches(&pattern, note)? && patterns.admit(note)? {
            paths.push(note.clone());
        }
    }
    Ok(paths)
}
```

Its `get_item` returns an error for a note that is not there, and nothing for one whose name `-Include` or `-Exclude` leaves out, as on a FileSystem drive. `item_exists` applies `-Include` and `-Exclude` the same way.

```powershell
New-Item -Path 'notes:\todo' -Value 'buy milk'
New-Item -Path 'notes:\trip' -Value 'pack'
Get-Item -Path 'notes:\t*' -Exclude trip    # todo
Get-Content -Path 'notes:\t*'               # buy milk, pack
Get-Item -Path 'notes:\nope'                # an error: notes:\nope does not exist
Remove-Item -Path 'notes:\t*'
```

## Making items

`New-Item` runs `new_item` with the type `-ItemType` gave and the `-Value` given. memfs follows a FileSystem drive's rules for an item already at the path, as measured in pwsh 7.6.7 and Windows PowerShell 5.1:

- Without `-Force`, an item already there is refused.
- With `-Force`, a file replaces a file, and `-ItemType Directory` where a folder is returns that folder as it is, with its contents.
- With `-Force`, `-ItemType Directory` where a file is changes nothing and returns nothing.
- A file is never made where a folder is, with or without `-Force`.

## Copying, moving and opening items

`Copy-Item` runs `copy_item`, `Move-Item` runs `move_item`, and `Invoke-Item` runs `invoke_default_action`. A copy or a move returns the item at its new path, which `-PassThru` writes. PowerShell passes the destination as the user gave it. When it names a folder that exists, whether the item goes inside is up to the provider.

memfs follows a FileSystem drive's rules, as measured in pwsh 7.6.7 and Windows PowerShell 5.1:

- A copy or a move onto a folder goes inside it.
- A copy of a file replaces a file of the same name. A copy of a folder onto a name that is already there is refused, and so is a copy of a file onto a folder of its name.
- `Copy-Item` without `-Recurse` copies a folder empty.
- A move of a file onto a file is refused unless `-Force` is given, which replaces the file. A move of anything else onto an item that is already there is refused even with `-Force`, and so is a move of a folder into itself. A move of an item onto itself changes nothing.

A copied or moved item has the same properties and security descriptor as before.

On a FileSystem drive, `Invoke-Item` opens a file with the program the system has for it. A memfs file has no such program, so memfs adds one to the file's `Invocations` property, which `Get-ItemProperty` reads. memfs refuses `Invoke-Item` on a folder.

## Item properties

Each of the eight `*-ItemProperty` cmdlets runs one method. Properties come and go as name and value pairs, `Vec<(String, PsObject)>`. The pairs a method returns are written as one object with those properties, and PowerShell adds `PSPath`, `PSChildName` and its other `PS` properties to it. No pairs means nothing is written.

| Cmdlet | Method | What it writes |
|---|---|---|
| `Get-ItemProperty`, `Get-ItemPropertyValue` | `get_property(path, names)`: the properties `names` lists, or all of them when the list is empty | the pairs returned |
| `Set-ItemProperty` | `set_property(path, values)` | with `-PassThru`, the pairs returned |
| `Clear-ItemProperty` | `clear_property(path, names)` | with `-PassThru`, the pairs returned |
| `New-ItemProperty` | `new_property(path, name, type_name, value)` | the pairs returned |
| `Remove-ItemProperty` | `remove_property(path, name)` | nothing |
| `Rename-ItemProperty` | `rename_property(path, name, new_name)` | with `-PassThru`, the pairs returned |
| `Copy-ItemProperty` | `copy_property(path, name, dest_path, dest_name)` | with `-PassThru`, the pairs returned, for the destination item |
| `Move-ItemProperty` | `move_property(path, name, dest_path, dest_name)` | with `-PassThru`, the pairs returned, for the destination item |

Every change runs under `ShouldProcess`, with the wording the registry drive uses, so `-WhatIf` and `-Confirm` work. A property the user turns down under `-Confirm` is left out of the pairs `set_property` gets. In `new_property`, `type_name` is what `-PropertyType` gave, or empty, and `pwrs::provider::call().force` is whether `-Force` was given; on the registry drive, `-Force` replaces a property that already exists, and memfs does the same.

memfs stores each item's properties in the order they were first set, and matches names without regard to case. A value comes back with the type it was set with, so an `Int32` set comes back as an `Int32`. memfs refuses `-PropertyType`, since it stores each value as given, and it refuses a name the item does not have.

## Security descriptors

`Get-Acl` and `Set-Acl` exist only on Windows, in pwsh and in Windows PowerShell. A security descriptor travels as SDDL text, the one-line form `Get-Acl` shows as `Sddl`:

- `get_security_descriptor(path, sections)` returns a `SecurityDescriptor { sddl, kind }`. `sections` is a `SecuritySections { owner, group, access, audit }` that lists the parts the call covers. `Get-Acl` covers the owner, the group and the access list, and the audit list too when given `-Audit`. The SDDL text may leave out the parts the call does not cover.
- `set_security_descriptor(path, sddl)` gets the SDDL text of each part the caller's descriptor has. `Set-Acl` runs `ShouldProcess` itself before the call, and with `-Passthru` it writes the descriptor it was given.

The `SecurityKind` sets which .NET object `Get-Acl` returns. PowerShell shows rights by name only for file and registry rules, so a provider whose items are files or keys gives that kind:

| `SecurityKind` | Object | Rights in `Get-Acl` |
|---|---|---|
| `File` | `FileSecurity` | by name, as file rights |
| `Directory` | `DirectorySecurity` | by name, as file rights |
| `RegistryKey` | `RegistrySecurity` | by name, as registry rights |
| `Other` | `Pwrs.ItemSecurity` | as a number, in each rule's `Rights` |

`$ExecutionContext.SessionState.InvokeProvider.SecurityDescriptor` makes an empty descriptor too. `NewOfType('MemFs', 'Other', 'All')` makes one of the kind named `File`, `Directory`, `RegistryKey` or `Other`, and `NewFromPath(path, 'All')` makes one of the kind the item has.

memfs gives a directory the `Directory` kind and a file the `File` kind. Each new item starts with `O:BAG:BAD:(A;;FA;;;BA)(A;;FR;;;WD)S:(AU;FA;FW;;;WD)`: the Administrators group owns it and has full control, everyone can read it, and a failed write by anyone is audited. memfs leaves the audit list out unless the call covers it. When it sets a descriptor, it leaves alone each part the SDDL text does not have, so a descriptor read without `-Audit` and set again still has its audit list.

## What the engine does for you

`ProviderBase` implements `NavigationCmdletProvider`, `IContentCmdletProvider`, `IPropertyCmdletProvider`, `IDynamicPropertyCmdletProvider` and `ISecurityDescriptorCmdletProvider`:

- it registers with `ProviderCapabilities.ShouldProcess` and calls `ShouldProcess` before each method that changes an item: `new_item`, `set_item`, `clear_item`, `remove_item`, `rename_item`, `copy_item`, `move_item`, `invoke_default_action`, `clear_content` and each property change. So `-WhatIf` and `-Confirm` work as on a FileSystem drive;
- it words each item and content change as an action, such as `Copy Item`, on a target such as `Item: mem:\a.txt Destination: mem:\b.txt`. `Set-Content` and `Add-Content` call `ShouldProcess` themselves before any batch reaches `write_content`. `Set-Content` on an item that exists calls it a second time, for `clear_content`, as on a FileSystem drive;
- it walks `Get-ChildItem -Depth` itself. It writes the children `get_child_items` returns without `recurse`, then the children of each container among them, down to the depth given, so `-Depth 0` writes the children alone. Each container's children are written together. With a filter, the containers it walks into come from a listing without the filter, so a container whose own name the filter leaves out is walked too. `-Recurse` without `-Depth` passes `recurse` to `get_child_items`, which returns the whole tree;
- it runs `has_child_items` without the call's filter and patterns, and adds the containers PowerShell walks into to `get_child_names`, as described in [the call's filter](#the-calls-filter-patterns--force-and-credential);
- it passes each batch `Set-Content` and `Add-Content` write to `write_content` as the batch comes, so an error reaches the command that wrote it. `Set-Content` empties an item that exists through `clear_content` before its first batch, so a provider implements both for `Set-Content` to replace an item. `Add-Content` moves the writer to the end of the item, which is where every batch goes already;
- it turns `get_content` output into an `IContentReader` that gives each result as the module returned it, including the `PSObject` around a wrapped one, so note properties on the wrapper reach `$drive:path`. The memfs example returns a directory as one `Pwrs.MemDir` object whose `Name` and `Entries` are such properties.

Each drive is a `PwrsDriveInfo`, a `PSDriveInfo` that stores the instance pointer and the drive's prefix. `ProviderBase` runs each operation on the instance of the drive its path is on. That is the drive PowerShell set for the operation, or, for a path with no drive set, the drive whose prefix starts the path. A path on no drive of this provider, such as `MemFs::x`, reaches no instance. `Get-Item`, `Get-ChildItem` and `Get-ItemProperty` report it as `PathNotFound`, and `Test-Path` returns `False`. A command that makes or writes an item there, such as `New-Item` or `Set-Content`, fails with the id `PwrsProviderNoDrive`.

## Errors

A method's `Err` reaches PowerShell as an error record with the error's id, category and message. Its target is the error's own, set with `with_target`, or else the path the command was on, drive included. The record's `FullyQualifiedErrorId` is the id followed by the command, and its exception is a `Pwrs.PwrsException`, whose `InnerException` has the error's details when it has any. For `New-Item` onto a file that is there, in pwsh 7.6.7 and Windows PowerShell 5.1 on Windows:

```powershell
$r = try { New-Item -Path 'mem:\docs\a.txt' -ItemType File } catch { $_ }
$r.FullyQualifiedErrorId    # ItemExists,Microsoft.PowerShell.Commands.NewItemCommand
$r.CategoryInfo.Category    # ResourceExists
$r.TargetObject             # mem:\docs\a.txt
$r.Exception.Message        # docs/a.txt already exists
```

Whether a command stops at the error depends on the command, as for any provider. At the default error preference, `New-Item`, `Get-Item`, `Get-ChildItem`, `Get-Content`, `Set-Content`, `Rename-Item`, `Move-Item`, `Remove-Item`, `Clear-Item` and `Invoke-Item` raise it as a terminating error, which `try` catches. `Get-ItemProperty` and `Copy-Item` write it and go on. An error from the content writer has the method's id as well.

## Between two drives

`Copy-Item`, `Move-Item`, `Copy-ItemProperty` and `Move-ItemProperty` can name a destination on another drive of the same provider, such as `Copy-Item mem:\a.txt mem2:\a.txt`. Each drive is its own Rust value, so those calls go to `copy_item_to`, `move_item_to`, `copy_property_to` and `move_property_to`, which take the source drive's value as `self` and the destination drive's as `dest_drive`. While such a call runs, no other operation can run on either drive. `ProviderBase` reserves the two drives in the same order every time, so two such calls going opposite ways never wait for each other. memfs copies and moves between drives by the rules it follows on one drive.

## Using it

```powershell
Import-Module ./target/pwrs/MemFs/MemFs.psd1
Get-PSProvider MemFs
New-Item -Path 'mem:\docs' -ItemType Directory
Set-Content -Path 'mem:\docs\a.txt' -Value 'hello'
Get-ChildItem -Path 'mem:\docs' -Recurse
Get-Content -Path 'mem:\docs\a.txt'
Set-ItemProperty -Path 'mem:\docs\a.txt' -Name Color -Value red
New-ItemProperty -Path 'mem:\docs\a.txt' -Name Size -Value 5
Get-ItemProperty -Path 'mem:\docs\a.txt' | Format-List Color, Size, PSChildName
Get-ChildItem -Path 'mem:\docs' -Recurse -Filter 'Color=red'    # a.txt
Get-Acl -Path 'mem:\docs\a.txt' | Format-List Owner, Group, AccessToString, Sddl    # Windows only
Rename-Item -Path 'mem:\docs\a.txt' -NewName 'b.txt'
Copy-Item -Path 'mem:\docs\b.txt' -Destination 'mem:\docs\c.txt'
Move-Item -Path 'mem:\docs\c.txt' -Destination 'mem:\'    # into the root folder
Invoke-Item -Path 'mem:\c.txt'                              # counts in its Invocations property
Get-ChildItem -Path 'mem:\' -Depth 1
Get-ChildItem -Path 'mem:\' -Recurse -Filter '*.txt'        # every .txt file, at any depth
Remove-Item -Path 'mem:\docs' -Recurse
New-PSDrive -Name mem2 -PSProvider MemFs -Root x    # a second, empty tree
Remove-PSDrive -Name mem2                           # frees it
New-Item -Path 'notes:\todo' -Value 'buy milk'      # a note, on MemNotes's drive
Get-Item -Path 'notes:\t*'                          # through expand_path
```

`Get-ItemProperty` and `Get-Acl` write, in pwsh 7.6 on Windows:

```text
Color       : red
Size        : 5
PSChildName : a.txt

Owner          : BUILTIN\Administrators
Group          : BUILTIN\Administrators
AccessToString : Everyone Allow  Read, Synchronize
                 BUILTIN\Administrators Allow  FullControl
Sddl           : O:BAG:BAD:(A;;FR;;;WD)(A;;FA;;;BA)
```

The `Sddl` lists the two access entries in the order .NET stores them, which is not the order memfs gave.

`examples/memfs/tests/MemFs.Tests.ps1` covers each of those in both hosts. A module with only providers exports no cmdlets; `Import-Module -Assembly` registers each provider from its `[CmdletProvider]` attribute in the shell assembly.
