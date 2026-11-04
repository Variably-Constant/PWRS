---
title: How To Ship A Second Library
weight: 15
---

A module sometimes ships a library that is not the module: a plugin another program loads from a folder, built from another package of the same workspace. `[package.metadata.pwrs.libraries]` builds it with the module and ships it in a folder of its own, and `pwrs::library_folder` gives the module a copy of that folder to load from.

A library you build goes here. A file you download goes through [`native-files`](How-To-Ship-A-Prebuilt-Native-File.md), and an executable through [`helpers`](How-To-Ship-A-Helper-Executable.md).

## Name the package

Each key of the table is a package of the module's workspace with a `cdylib` target:

```toml
[package.metadata.pwrs.libraries.pwrs-example-hello-plugin]
folder = "plugins"
```

| Key | Value | What it does |
|---|---|---|
| `folder` | a folder name | the library ships in `runtimes/<rid>/<folder>/`, beside `native` |
| `features` | Cargo feature names | the features its build turns on |
| `default-features` | `true` or `false` | whether its build takes the package's default features; `true` when left out |

The package is built by itself, so it takes these and not the module's. A package that is not in the workspace, has no `cdylib` target, or is the module's own package stops the build, named. So does a file already in the folder under the library's name.

## Ship it on some platforms

Entries under a `cfg(...)` key apply only to a target whose cfg satisfies it, as `rustc --print cfg --target <triple>` prints it, the way `native-files` scopes its own:

```toml
[package.metadata.pwrs.libraries.'cfg(any(target_os = "linux", target_os = "freebsd"))'.gst-tracer]
folder = "gstreamer-1.0"
```

A package no entry applies to for the target is not built for it. A package two entries name for one target stops the build, named.

## What the build does with it

`cargo pwrs build`, `test` and `publish` build the package for the module's target and profile, after the module, and copy its library to `runtimes/<rid>/<folder>/`. For a Windows target, when the build writes `.pdb` files, its `.pdb` goes beside it.

The library counts as the module's in every check the module's own library has:

- Its package and every crate it links are held to the module's [allowed licenses](How-To-Publish.md#license-notices), and one that fails stops the build.
- `runtimes/<rid>/THIRD-PARTY-NOTICES.txt` lists them after the module library's, under its own heading. `[package.metadata.pwrs.license-files]` entries reach them too.
- What it was compiled to require is held to `cpu-features`, when it has a list to read. That list comes from PoWerRuSt, so a library that does not link PoWerRuSt has none.

`cargo pwrs merge` copies each `runtimes/<rid>` folder whole, `publish` packages the module folder as built, and `sign` signs every library under the folder, so all three include it.

## Load it from Rust

```rust
let plugins = pwrs::library_folder("plugins")?;
```

`pwrs::library_folder` takes the entry's `folder` and returns the path of a copy staged for the process, at `<staging>/<pid>/<key>/libraries/<folder>.<mark>/<folder>`, so the path's last component is the folder's own name. The copy is whole before its path is returned, and is named for the files in it. Load from this path and never from the module folder: a loaded library goes on using its file, and the next build clears that folder.

The folder holds the library and nothing that needs the module, so another program can take the path, as a helper executable that sets a plugin search path for what it starts. A name the module ships no folder of for this platform is an error naming the folder and where it looked.

hello ships `examples/hello-plugin` this way. `Get-HelloLibraryFolder plugins` returns the path, and `examples/hello/tests/Libraries.Tests.ps1` checks the folder, the notices and the staged copy, and loads the library from that copy and calls it.
