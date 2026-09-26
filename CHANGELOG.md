# Changelog

What changed in each version. The repository ships as a single root
commit that is rewritten on every release, so this file is the record
of what came before it; the commit log is not.

The five crates are published together at one version: `PoWerRuSt`
(imported as `pwrs`), `pwrs-sys`, `pwrs-macros`, `pwrs-build` and
`cargo-pwrs`.

## 0.2.3 - 2026-09-25

### Added

- `proxy_enter_shared`, host-table entry 75: the shared form of
  `proxy_enter`, which `PsProxy::with` now takes, so a `with` borrow
  nests inside a `&self` method or a property read on the same thread
  and is refused only inside a `&mut self` method or a `with_mut`
  borrow. The table only grows, so a module built for 0.2.2 loads on
  this runtime.
- A `bundled-modules` entry can be a table: `path`, the crate directory
  the string form names, with any of `on-import-failure`, `features`,
  `default-features`, `license-files` and `cpu-features`.
  `on-import-failure = "warn"` makes the module's script write a warning
  naming the bundled module and its error when it cannot be imported,
  and import the module without it; `"stop"`, the default and the string
  form's behavior, fails the import. `features` and `default-features`
  reach the bundled crate's build. `license-files` and `cpu-features`
  cover the bundled library, beside the bundled crate's own settings, so
  a module can supply them for a crate it bundles from another project's
  checkout; one crate and version supplied from both places stops the
  build. The hello example's `Calc` entry warns, and its suite imports a
  copy whose `Calc` cannot load.

### Changed

- `cargo pwrs publish` checks each bundled module's library as it checks
  the module's own: crates it links without a license text, and
  extensions it requires beyond its target's baseline that nothing
  declares, refuse the package, naming the bundled module and the entry
  setting that lets it through. Earlier versions checked the bundling
  module's library alone.
- A `bundled-modules` entry that is neither a string nor a table, and a
  table key or value the entry does not take, stop the build, named.
  Earlier versions skipped an entry that was not a string.
- The crates require their siblings at exactly their own version:
  PoWerRuSt requires `=0.2.3` of pwrs-sys and pwrs-macros, and
  cargo-pwrs and pwrs-host `=0.2.3` of pwrs-build. Earlier releases
  wrote `^` requirements, so a lock refresh could pair PoWerRuSt 0.2.0
  with pwrs-sys 0.2.2, whose larger host table it does not compile
  against; a module that holds an earlier release pins all three.
- `Cargo.lock`, which `cargo install cargo-pwrs --locked` builds from,
  takes the newest compatible releases: cc 1.5.1, find-msvc-tools
  0.1.14, smallvec 1.16.2, zerocopy and zerocopy-derive 0.8.59, and
  cudarc 0.19.10 for the optional flynnel feature. Every direct
  dependency already named its newest stable release.

### Fixed

- A proxy method called with its own receiver as a by-value argument,
  such as `$c.CompareTo($c)` on a proxy class, failed with "in use by a
  call already running on this thread": the call held the object's gate
  while the argument was read back through the proxy's getters, and the
  gate refused every re-entry. The gate now tells a shared entry (a
  property read, a `&self` method, a `with` borrow), which nests on the
  thread inside one, from an exclusive entry (a `&mut self` method, a
  `with_mut` borrow), which is refused while anything is inside and
  refuses everything while it runs; a `&mut self` method called with
  its own receiver by value stays refused, with the same message. The
  generated shell passes each method's receiver kind, which the
  descriptor carries as `mutable` (read as exclusive when a descriptor
  does not carry it), and a `&self` method runs on a shared reference.
  In the hello example `Hello.Counter` gains `SameAs`, a `&self` method
  taking a counter by value, its suite calls `SameAs` and `Absorb` with
  their own receiver, and `Test-RustSeriesHold` gains `-Exclusive`, so
  the suite reads a held series from the holding thread under each
  kind of hold.
- A proxy class's public constructor whose Rust half returned an error,
  such as a failed `[Type]::new(...)`, left an object no constructor
  had run on, and when the collector finalized it the finalizer threw
  on the finalizer thread, which ended the process with
  `System.ArgumentNullException` from `Pwrs.ProxyBase.ReleaseInstance`.
  The finalizer now leaves such an object alone, since it holds no
  value to free. In the hello example `Hello.Counter`'s constructor
  refuses a label holding `=`, which `Parse` could not read back, and
  the suite collects a counter whose construction failed in a child
  host and checks the host goes on.

## 0.2.2 - 2026-09-25

### Added

- `[package.metadata.pwrs] bundled-modules`, other PWRS crates whose
  modules ship inside this one. `cargo pwrs build` builds each with the
  same tool for the same target and profile, `--locked`, into
  `target/pwrs-bundled/<crate directory>/` under the bundling module's
  target directory, refuses it unless its `Pwrs.Bootstrap.dll` and
  `Pwrs.Runtime.dll` match the bundling module's byte for byte, and
  copies its folder to `<Module>/<Name>/`. The generated script imports
  each into the session before its own shell, `-Global` and
  `-DisableNameChecking`, and leaves one the session already holds as
  it is, which is what lets the import succeed on PowerShell 7 at all.
  `publish` carries the folder inside the package, and `merge` copies a
  bundled module's `runtimes/<rid>` into the destination's copy of it.
  The hello example bundles the calc example, and `Bundled.Tests.ps1`
  proves the folder, the byte-identical runtime, the import from the
  bundled folder and the reuse of a module imported first, in both
  hosts.
- `benches/dynamic_params.ps1`, which prices a cmdlet's dynamic
  parameters per call against compiled C# cmdlets and splits PWRS's
  share into the bound-parameter table, the call into the library and
  the hook's reads; and in the hello example `Get-RustStaticReading` and
  `Get-RustBlindReading`, `Get-RustReading` with no hook and with a hook
  that reads nothing and adds nothing, which the bench times it against,
  and `Get-RustTableEntry`, which reads one key of a table as it was
  passed.
- `benches/bulk_array.ps1`, which prices a large array's binding in
  each form it can arrive in (a bare `byte[]`, the same array wrapped
  in a `PSObject`, an `object[]` of the values boxed) through each way
  a parameter can take it, in both hosts, with the busy cores of every
  other process printed beside each figure. `docs/PERF.md` carries its
  table.
- `?` on a `std::collections::TryReserveError` gives an error record
  with id `PwrsOutOfMemory` and category `ResourceUnavailable`, the
  pair `PsMemory::zeroed` already raised. A cmdlet that reserves
  memory sized by its input with `try_reserve` now reports a refused
  allocation as an error and the session carries on; the infallible
  forms end the process, which no boundary can catch.
- `pwrs::text::try_from_utf16` and `try_from_str16`, the forms of the
  UTF-16 readers that reserve before they allocate.
- A module compiled for x86-64 extensions its CPU lacks refuses to
  import with the missing ones named, instead of ending the session at
  the first such instruction. A library built with
  `-C target-cpu=native` carries them throughout. `export_module!` now
  emits the list the crate was compiled for as the data export
  `pwrs_cpu_requirements`, beside `pwrs_cpuid` and `pwrs_xgetbv`,
  whose bodies are only those instructions, and the runtime reads and
  checks it before any other export runs. `cargo pwrs build` warns about
  requirements beyond the target's own baseline, and `cargo pwrs
  publish` refuses them unless `[package.metadata.pwrs] cpu-features`
  lists them. A module now needs rustc 1.88 or later for the naked
  functions.
- `pwrs::cpu`: whether the CPU and the operating system offer each
  x86-64 extension, which ones the library was compiled for, and `has`,
  the predicate to dispatch kernels on. `PWRS_CPU_MAX` caps it at a
  psABI level, and the import check applies the same cap, so one
  machine can run every tier of a module.
- A `PsObject` method called from a worker the module started (`get`,
  `set`, `call`, `pin`, `type_name`, `type_tag`, and `PsType`'s calls)
  returns an error with id `PwrsOffThread` in a debug build and under
  `cargo pwrs test`, which sets `PWRS_THREAD_CHECK=1` for its hosts.
  `PsObject` is `Send`, so the compiler lets a worker capture one, but
  such a call attaches the worker to the .NET runtime and runs the
  engine's member binder on a thread no runspace belongs to. A release
  build does not check.
- `PsProxy<T>`, a parameter holding a `#[psclass(proxy)]` object of
  class `T`. It is declared with `T`'s CLR type, so the binder accepts
  that class alone, from the pipeline, a variable, or through commands
  that know nothing of it. `with` and `with_mut` lend the value in place
  under the object's gate, so a proxy with Rust-only state, which cannot
  be read back by value, can be taken by a cmdlet; writing a `PsProxy`
  writes the same object. The borrow crosses through two host-table
  entries, `proxy_enter` and `proxy_exit`.
- `#[psclass(proxy, native_bytes = path)]` names a `fn(&Self) -> usize`.
  The object reports its answer with `GC.AddMemoryPressure` when made,
  again after each method call and each `with_mut`, and withdraws it
  when the value is freed, through the optional export
  `pwrs_proxy_bytes`.
- `Pipeline::stream_from_thread_until`, which hands the worker a
  `StopSignal` as well as the channel. It is set when the pipeline
  thread stops draining, so a worker that computes without sending can
  return instead of holding a stopped pipeline until it is done.
- `cargo pwrs build` writes the license notices a module owes.
  `runtimes/<rid>/THIRD-PARTY-NOTICES.txt` beside each native library
  lists every crate the library links with the license it declares and
  the license files its source carries, word for word, and the module
  folder's own covers the managed runtime and the .NET Framework
  assemblies a module with hand-written C# ships. A crate whose license
  expression the allowed licenses do not satisfy stops the build; the
  defaults are permissive licenses, and `[package.metadata.pwrs]
  allowed-licenses` adds to them. `cargo pwrs publish` refuses a module
  whose library links a crate whose source carries no license file.
- Each published crate carries the repository's `LICENSE`, so a module
  built on them can quote it.
- `--target <triple>.<glibc>`, as cargo-zigbuild spells it
  (`x86_64-unknown-linux-gnu.2.35`), builds the library through
  `cargo zigbuild` against that glibc's symbols, so a module can load
  under a pwsh running on an older glibc than the building machine's,
  such as the powershell snap's.
- Helper executables. `[package.metadata.pwrs] helpers` names some of
  the package's `[[bin]]` targets, and `cargo pwrs build` ships each
  beside the native library in `runtimes/<rid>/native/`, built for the
  same target and profile, so `merge` and `publish` carry them; a name
  that is no `[[bin]]` target stops the build. `pwrs::helper_path(name)`
  answers where to start one: a copy staged for the process beside the
  staged library, made on the first request for the helper's bytes, so
  a running helper holds no file in the module folder. It crosses
  through a new host-table entry, `helper_path`. Outside Windows the
  copy is readable and executable by its owner whatever mode the shipped
  file has, since a `.nupkg` records no Unix mode for its entries.
- `#[param(set = ["Path", "LiteralPath"])]` puts a parameter in several
  parameter sets and no others: the shell declares one `[Parameter]`
  per set, so the binder refuses the parameter beside one of any other
  set, and help writes a syntax line per set. The descriptor carries a
  parameter's sets as `sets`; `cargo pwrs` still reads the single `set`
  an older crate writes.
- `#[param(clr = "byte[]")]` on a `PsObject` parameter declares it as
  that CLR type, so the binder coerces to it and chooses between
  parameters by it, while the value still crosses as a handle and is
  read in place. A byte array piped to a compressor-shaped cmdlet then
  arrives whole, and a piped file goes on to a `-LiteralPath` bound from
  its `PSPath` instead of stopping at an `object` parameter.
- `#[param(allow_empty_collection)]` declares `[AllowEmptyCollection]`,
  so a mandatory collection parameter takes an empty array instead of
  the binder refusing it.
- `[package.metadata.pwrs.license-files]` supplies the license file a
  linked crate's package lacks, keyed `"name@version"`, as a path in the
  module's own repository. The notices quote it for that crate and
  `publish` accepts the module; an entry for a crate or version the
  library does not link, for a crate that ships its own license file,
  or pointing at a missing or empty file, stops the build.

### Changed

- `stream_from_thread` waits on its channel 50 ms at a time and reads
  the stop flag between waits, so a stop is seen while the worker sends
  nothing. It used to wait for the worker's next send.
- Every dependency requirement names its newest stable release:
  syn 3, which moved the receiver's reference form into
  `ReceiverKind`, so `#[psmethods]` reads it there; libloading 0.9;
  criterion 0.8 for the benches, whose `black_box` is now
  `std::hint::black_box`; and serde, serde_json, quote and proc-macro2
  at their newest 1.x.
- `cargo pwrs test` streams each host's Pester output as the suite
  writes it. It used to print a host's output when that host exited,
  so a test that never returned left the run showing only
  `pwrs: Pester in pwsh`. `pwrs_build::pwsh` gains `stream_pwsh_script`
  and `stream_winps_script`, which give the script an empty stdin, as
  the capturing pair does.
- `tools/hot_reload_gate.ps1` and `tools/coload_gate.ps1` take
  `-Target`, a triple they hand to `cargo pwrs build --target`, so the
  modules they build load under a pwsh on an older glibc than the
  building machine's.
- The license's copyright line, in the repository and in each crate,
  names Mark Newton.
- A cmdlet's dynamic-parameter hook costs less per call. The generated
  `GetDynamicParameters` builds one table for the hook, each value out
  of the PSObject the binder may have wrapped it in, and the runtime
  pins that table for the call instead of copying it into a second; the
  hook's table now compares names ignoring case, as
  `MyInvocation.BoundParameters` does. A hook that adds nothing hands
  the engine `$null`, which it reads as no dynamic parameters and prices
  below an empty table. And a `PsHashtable` or `PsReadOnlyTable` read
  from Rust (`get`, `set`, `contains`, `remove`) is answered by the
  dictionary itself in the host, instead of by PowerShell's adapter and,
  for `get`, reflection; a missing key on any table reads as `$null`.
  Measured on a Windows desktop with `benches/dynamic_params.ps1`,
  paired within rounds, `Get-RustReading` less the same cmdlet with no
  hook: 3,414 ns to 1,900 ns per call in pwsh 7.6.6, 18,757 ns to
  7,649 ns in Windows PowerShell 5.1; `GetDynamicParameters` called
  directly, less a C# hook returning an empty table: 1,993 ns to
  1,412 ns and 10,227 ns to 2,709 ns. The bound-parameter table itself
  was 162 and 308 ns of that before. `docs/PERF.md` carries the whole
  table with the load each run carried.
- A dynamic-parameter hook's answer crosses as one managed string, a
  line per parameter joined by U+0003 with seven cells joined by U+0001
  and validate-set values by U+0002, and `$null` for none, instead of an
  `object[]` of `string[7]` rows built through seventeen host crossings
  per parameter; the runtime splits it, and a line without its seven
  cells is an error naming the count where it was skipped before.
  Measured on the same Windows desktop, `GetDynamicParameters` called
  directly, PWRS less a C# hook adding the same parameter: 4,536 ns to
  2,200 ns per added parameter in pwsh 7.6.6 and 8,134 ns to 3,507 ns
  in Windows PowerShell 5.1; `Get-RustReading -Kind temperature` less
  the same cmdlet with no hook, end to end: 4,930 ns to 3,998 ns and
  28,397 ns to 18,021 ns. `docs/ABI.md` states the format.

### Fixed

- A provider's content reader handed the engine each `get_content`
  result's base object, so the properties a `PSObject` carries on its
  wrapper, a deserialized object's among them, read as `$null` through
  `$drive:name` while `(Get-Item drive:name).Value` kept them. The
  reader now hands each result over as the module returned it. The
  memfs example reads a directory through `$mem:path` as one
  `Pwrs.MemDir` object whose `Name` and `Entries` are note properties,
  which its suite checks.
- A module imported from several runspaces at once, in a process that
  had not imported it yet, failed in some of them: each ran the
  generated script's bootstrap copy and the loader's staging over the
  same files, so one loaded a copy another was still writing, and on
  Windows PowerShell every later import in that process then failed to
  load the runtime from the staging folder. The script now runs under
  a mutex named for the process, the loader loads under a lock of its
  own, and a staged copy is written under a temporary name and renamed
  into place, so a path that exists is whole. The hello suite imports
  the module cold from eight runspaces at once in a child host of each
  edition, twice, with every import succeeding.
- How To Use Threads said `PsObject` methods could be called on
  workers; it now says a worker builds plain Rust values and leaves
  `PsObject` to the thread the cmdlet runs on.
- The README's row for threads split at the closure's `|tx|` on GitHub,
  which read it as a column break and dropped the rest of the row. The
  pipes are escaped.
- Reading a managed string, array, hashtable, `BigInteger` or
  `SecureString` into Rust allocated the copy infallibly, as did every
  string parameter, so a value too large for the allocator ended the
  host process. They now reserve first and return `PwrsOutOfMemory`.
- `PsSecureString::reveal` could free a buffer holding part of the
  secret without zeroing it, when the string grew between the read of
  its length and the read of its text. Every buffer it gives up is now
  wiped first.
- A proxy disposed on one thread during a property read or method call
  on another had its value freed under the running call. `Dispose` now
  waits for the object's gate, and on the thread holding it, from a
  script the call runs, the value is freed when the call ends.
- A property read or method call reaching a proxy from inside a call
  already running on the same thread was handed a second reference to a
  value in use. It is now refused with a `PwrsException`.
- A byte array written by any cmdlet reaches the next one wrapped in a
  PSObject, and binding it to a `byte[]` parameter converted it element
  by element: 8.4 s in pwsh 7.6 and 24 s in Windows PowerShell 5.1 for
  32 MiB, against milliseconds unwrapped. Every array-typed parameter
  now carries `[Pwrs.UnwrapArray]`, a transformation the binder runs
  first, which hands it the wrapped array itself.
- A module's dynamic parameters did not complete. The generated
  `GetDynamicParameters` handed the hook `MyInvocation.BoundParameters`,
  which completion leaves empty, so during `TabExpansion2` the hook saw
  nothing bound and offered nothing. It now also hands the hook every
  parameter set on the cmdlet object, which completion does set.
- A warning written with `warning!` under `-WarningAction
  SilentlyContinue` was not written at all, so `-WarningVariable` stayed
  empty where `Write-Warning` fills it, and an information record
  written with `information!` under the default `$InformationPreference`
  never reached `-InformationVariable` or a `6>` redirection. The
  runtime now counts a warning or information record as kept wherever
  the engine keeps one written by its own cmdlets; verbose and debug
  records are kept only where they are shown, as before.
- A module fresh from `cargo pwrs new` ended its first `cargo pwrs test`
  with a surface-check finding against its own cmdlet: the scaffolded
  suite never piped anything into `-Name`, which the cmdlet declares as
  taking pipeline input. The suite now pipes two names in as well.

## 0.2.1 - 2026-09-23

### Added

- Hand-written C# can use spans on Windows PowerShell. The
  `netstandard2.0` compile references the .NET Framework builds of
  `System.Memory` 4.6.3, `System.Buffers` 4.6.1 and
  `System.Runtime.CompilerServices.Unsafe` 6.1.2 beside
  `System.Numerics.Vectors`, and a module with hand-written C# ships
  all four in its `netstandard2.0` folder, which its script copies
  beside the staged shell. Six files of SIMD code written for a .NET
  SDK project targeting .NET Framework and .NET 8 failed on that half
  with four `ReadOnlySpan<>` errors, and compile now. Hello's
  `Measure-RustHybridDot` walks its arrays in `Vector<int>` steps
  through `MemoryMarshal.Cast`. The copy costs the first import in a
  Windows PowerShell process: on hello, 5.8 ms by minimum and 8.4 ms by
  median over 21 fresh processes, with a warm re-import unchanged.

### Changed

- The PowerShell 7 half of every module is compiled against .NET 8's
  reference pack, `Microsoft.NETCore.App.Ref` 8.0.31, and the
  `System.Management.Automation` 7.4.0 reference, both fetched from
  NuGet, instead of the reference set of the pwsh doing the build.
  Every machine now emits the same references, and hand-written C# on
  PowerShell 7 can use only what .NET 8 and PowerShell 7.4 have: code
  calling an API only .NET 9 or 10 has stops compiling. The folder
  keeps its name, `net10.0`. On a PowerShell 7 whose .NET is older
  than 8, the module's script stops with a line naming the PowerShell
  it needs.

- `System.Numerics.Vectors` moves from 4.5.0 to 4.6.1, and the
  toolchain fetches four more packages. Each package version is
  fetched into a folder named for it (`simd-4.6.1/`) and the scripts
  into one named for their text, so an existing toolchain fetches its
  packages once more, and `cargo-pwrs` 0.2.0 on the same machine keeps
  reading its own unversioned folders, which this version never
  writes.

### Fixed

- A module built on one PowerShell 7 did not load on an older one,
  because its PowerShell 7 half referenced the .NET of the pwsh that
  built it. Measured by a consumer: a module built on pwsh 7.6.6 failed
  on 7.5.11, 7.4.20 and FreeBSD's 7.5.5 with "Unable to find type
  [Pwrs.Bootstrap.Loader]", which broke a published module there, and
  one built on FreeBSD's 7.5.5 failed on 7.4.20 and referenced
  `System.Private.CoreLib` directly, since FreeBSD's pwsh ships
  implementation assemblies. `cargo pwrs merge` keeps the first
  folder's managed half, so the machine that built it set the floor
  for every platform. The pinned references above remove the cause.

- Hand-written C# using `System.Numerics.Vector<T>` compiled for
  Windows PowerShell and failed there when it ran, with a
  `FileNotFoundException` for `System.Numerics.Vectors` 4.1.3.0: the
  compile referenced the package's reference assembly, whose version no
  build in the package carries, .NET Framework has no
  `System.Numerics.Vectors` of its own, and nothing beside the staged
  shell could answer. The compile now references the .NET Framework
  build a module ships, at the same version.

- Hand-written C# guarded the way a .NET SDK project guards it
  compiled differently under `cargo pwrs`, because only `NET`,
  `NET10_0` and `NETCOREAPP`, or `NETSTANDARD` and `NETSTANDARD2_0`,
  were defined. SIMD code keeping its `System.Runtime.Intrinsics` paths
  under `NET8_0_OR_GREATER` built without them and no warning said so:
  compiled with those defines, the six files above referenced no
  intrinsics type, and with the SDK's they reference 17. Both compiles
  now define what the SDK defines for net8.0 and netstandard2.0, the
  `OR_GREATER` symbols included, and nothing else; `NET10_0` is not
  among them.

- Two modules imported at the same moment from two runspaces of one
  process could load one module's native library under the other's
  shell, when their module folders hash alike. A shell runs from a
  staged copy whose folder is named by a 31-bit hash of the module
  folder, and it found its module folder through the loader's one
  table from staging folder to module for the whole process, where two
  such modules share an entry and the later import overwrites the
  earlier one's. The module's script now hands its shell the folder it
  was imported from before importing it, and the shell asks the loader
  only when nothing was handed over. `Pwrs.Bootstrap`, the runtime and
  the native ABI do not change. Found by an audit of what two modules
  in one process share, not by a failure.

- `cargo pwrs test` reported a Pester that would not load as the bare
  `Import-Module` error, so a scaffolded project on a host whose
  Pester is a OneDrive placeholder failed with "The cloud file
  provider is not running" and nothing tying it to the test run. It
  now writes one line naming the host, each Pester it tried, what the
  import said, and that `PWRS_PESTER_PATH` overrides the choice, and
  exits 2. How To Fix A Failure has the message.

## 0.2.0 - 2026-09-23

The minor version moves for one change in what two modules can rely
on. On Windows PowerShell, where the first PWRS module's runtime
serves every module imported after it, when one of two modules was
built by `cargo-pwrs` 0.1.8 or earlier and the other by this one, the
one imported second cannot be imported if it declares classes or
enums, as described under Fixed. A module declaring neither imports
after either, and the module imported first keeps working. Rebuild
every PWRS module one Windows PowerShell session imports with one
`cargo-pwrs`. PowerShell 7 gives each module its own runtime and is
not affected. The Rust API only adds, and the native ABI does not
change.

### Added

- `Pipeline::invocation()` hands back the running cmdlet's
  `MyInvocation`, the engine's `InvocationInfo`, with one dynamic
  member access and no vtable entry. It is set before `begin`, so any
  phase can read where the command stands: `PipelinePosition`, counting
  from 1, and `PipelineLength`, which counts commands, so an expression
  at the head of a pipeline is input and not counted. That is what a
  set of cooperating cmdlets needs to tell that their neighbors are
  their own. One behavior is worth knowing before relying on it,
  measured on both editions: begins run left to right, but a command's
  queued input is processed right after its own begin, so when an
  upstream begin writes output, a command can process it before a
  later command has begun.

- A copied `#[psclass]` takes `#[psmethods]` statics, and a `new`
  returning `PsResult<Self>` is its constructor, reached as
  `[Ns.Type]::new(...)`. A copied object carries its fields rather
  than a Rust value, so a value type no longer has to become a proxy,
  and have every property read cross the boundary, just to be made
  without a `New-` cmdlet. `new` builds the value in Rust and the
  object is made from it field by field. A method taking `&self` on a
  copied class is refused by `cargo pwrs build` with the reason.

  Declaring a `new` also removes the public parameterless constructor
  C# otherwise supplies, which fills CLR zeros: `[T]::new()` on such a
  type made an object the class never meant to exist, which bound as
  a valid value and failed far from where it was made. A class that
  declares no `new` keeps that constructor, as it always had. Rust has
  no overloading, so one `new` with `Option` arguments answers every
  arity, and one starting from `Self::default()` gives a parameterless
  constructor that starts from `Default`. A constructor has no
  pipeline, so it refuses rather than warns.

### Fixed

- Two PWRS modules in one Windows PowerShell 5.1 session built each
  other's objects. A class id is its class's position in its own
  module's `export_module!` list, so every module numbers its classes
  from 0, and `factory_new` looked the id up in one static table per
  copy of `Pwrs.Runtime`. PowerShell 7 gives each module its own load
  context and so its own copy, but Windows PowerShell has one load
  context per process, where the first module's runtime serves every
  module after it: the module imported second replaced the first one's
  factories. The first module's copied objects and enum values then
  came back as the second module's types, read with the second
  module's field layouts, and a proxy of the first module's value was
  bound to the second module's library, which read it, called it and
  freed it as a type it is not. Measured with two modules built outside
  this repository: in one order an object of the first module came
  back as a type of the second, and in the other a call threw an
  `OverflowException`.

  Each module now owns its factory table. On Windows PowerShell each
  module's native library is handed its own copy of the host table,
  whose `factory_new` answers from that module's factories, for the
  first load and every reload; on PowerShell 7 the table stays one per
  copy of the runtime, which was already one per module. The table
  takes no lock: its only writer is the module's shell, from its type
  initializer, which finishes before anything can reach the module.
  The native ABI and the Rust crates do not change, so a module gets
  the fix by rebuilding with this `cargo-pwrs`.
  `Pwrs.NativeModule.FactoriesPerModule` answers `true` on a runtime
  with the fix and is absent before it, for a module that must decide
  whether two PWRS modules can share a Windows PowerShell session.

  `tools/coload_gate.ps1` builds one fixture twice under two names and
  imports both into one session in each order, checking a copied
  object, a proxy call and `Dispose`, and an enum from each. It fails
  on the unfixed runtime under 5.1 and passes under both editions with
  the fix. The hot reload gate beside it now also checks that each
  reload's objects come from that reload's own shell.

  On Windows PowerShell, when one of two modules was built by
  `cargo-pwrs` 0.1.8 or earlier and the other by this one, the one
  imported second fails to import if it declares classes or enums,
  with `The type initializer for
  'Pwrs.Modules.<Name>.PwrsModule' threw an exception`, over a
  `MissingMethodException` the engine does not print, where before it
  shared the first module's factories silently. A module declaring
  neither imports after either, and the first module keeps working.
  Measured in every order of a 0.1.8 and a 0.2.0 module, one declaring
  classes and an enum and the other neither, on Windows PowerShell
  5.1.26100.9444 and pwsh 7.6.6, where every order imports. Rebuild
  both with the same `cargo-pwrs`; How To Fix A Failure has the detail.

- The surface check's file was named by process alone, so two PWRS
  modules loaded in one host process wrote the same file and each
  replaced the other's table: the report saw only whichever module
  released a cmdlet last. Each native image now writes
  `<pid>-<image>.tsv`, and `cargo pwrs test` sums them by cmdlet name
  as before. Test time only; found by the audit of what two modules in
  one session share.

- `cargo test --workspace` aborted on a Linux host whose pwsh is the
  snap: `pwrs-host`, the workspace's in-process test host, started the
  snap's .NET without the ICU it runs on, and .NET ended the process
  with "Couldn't find a valid ICU package installed on the system".
  The snap's pwsh finds the ICU the snap bundles through an RPATH into
  the snap, and its launcher names the version in
  `CLR_ICU_VERSION_OVERRIDE`; a process outside the snap has neither.
  The host now loads the snap's ICU by full path before the runtime
  starts and sets the override unless it is set. Measured on Ubuntu
  24.04 with the powershell snap's pwsh 7.6.5, whose ICU is 70.1
  beside the system's 74.2.

- `PsResult<Self>` in a `#[psmethods]` signature did not compile,
  though it is the spelling the documentation gives for a constructor:
  the return and argument types were written inside the generated
  `impl ... for MethodsCollector<T>`, where `Self` names the collector
  and not the class. `Self` is now resolved to the class wherever a
  type is written, `other: Self` included.

## 0.1.8 - 2026-09-22

### Added

- `PsReadOnlyTable` hands PowerShell a table script reads through
  both `$t.key` and `$t['key']` and cannot write through either.
  `PsReadOnlyTable::over` takes an object the caller already holds,
  so the keys are copied once and the values are shared.

  The engine exposes a key as a property only for the non-generic
  `IDictionary` carried on a type's public surface, which is why
  `Hashtable` and `OrderedDictionary` answer `$t.key` while
  `Dictionary` and `ReadOnlyDictionary` answer `$null`. Losing one
  access form silently is worse than the sharing it would guard
  against, so the type carries the interface publicly and refuses
  every mutating member instead. Both write forms reach the indexer,
  and the refusal raises rather than being ignored.

  It holds the source rather than copying it, so enumeration keeps
  the source's order and making one costs nothing per key. That makes
  it a view and not a snapshot: a change made through the source
  shows through. A nested table comes back wrapped as well, from the
  indexer, from `Values` and from enumeration, so the refusal reaches
  all the way down; each such read builds a wrapper, so two reads of
  one key match by content and not by reference.

- `benches/dynamic_reads.ps1` times one property read taken from Rust
  against the same read taken from PowerShell, over the same objects
  in one process, and `Measure-RustPropertyReads` in the hello example
  is the cmdlet it drives. Measured figures are in `docs/PERF.md`: the
  crossing beats a name PowerShell resolves at run time, by about 3.1x
  on a PSCustomObject and 1.4x on a CLR object, and loses to a name
  compiled into the member access, which PowerShell reaches through a
  cached call site.

- `#[transform(cmdlet = "Get-Thing", parameter = "Size")]` on a
  `fn(&PsObject) -> PsResult<PsObject>` runs before the binder coerces
  the argument to the parameter's declared type and before
  validation, which is where nothing else can run: a `long` parameter
  accepts `2MB`, and a refusal is a binding failure naming the
  parameter rather than an error the cmdlet wrote. Listed under
  `transforms` in `export_module!`, it generates an
  `ArgumentTransformationAttribute` on the parameter and reaches Rust
  through `pwrs_transform_invoke`, bound optionally so a library
  built before it existed still loads. No instance exists yet, so a
  transform takes no `Pipeline` and cannot write to a stream; a value
  it does not recognize is handed back for the binder to coerce or
  reject.

- `#[psenum(clr = "System.ConsoleColor")]` mirrors a CLR enum that
  already exists rather than declaring one. Nothing is generated, it
  takes no class id, and it is not listed under `enums:`. The
  parameter is declared as that CLR type, so the binder converts its
  member names, completes them, and rejects the rest before the body
  runs, and a value written back is that type and not a number. A
  variant list narrower than the CLR enum's members is allowed: a
  member the Rust enum does not name binds and then fails in `FromPs`.
  Vtable entry 71, `enum_new`, builds the value through
  `Enum.ToObject` with the name resolved the way a type literal is.
  `name` and `clr` together are a compile error.

- `cargo pwrs test` reports what the suite left unexercised of what
  the module declares: a cmdlet no test invoked, a cmdlet that ran
  without ever asking though it declares `SupportsShouldProcess`, and
  a parameter declaring `ValueFromPipeline` that nothing was ever
  piped into. The engine keeps both promises whether or not the body
  holds up its end, so neither breaks loudly: `-WhatIf` on a cmdlet
  that never asks does the thing it was supposed to describe.

  Neither is decidable from the declaration, so the run is the
  evidence. `pwrs::surface` records what each phase saw while
  `PWRS_SURFACE_DIR` is set, each host writes `<pid>.tsv` there, and
  the tool reads both against the descriptor. Unset, which is every
  production call, a phase pays one relaxed load and a branch. A
  finding never fails the command: it is either a missing test or a
  promise the body does not keep, and only the author knows which.

- `PsErrorRecord` reads a `System.Management.Automation.ErrorRecord`
  by its parts: `category` as the typed `ErrorCategory`, `error_id`,
  `message` and `target`, which is `$null` when the record carries
  none. A record taken as a parameter, handed down the pipeline,
  collected by `-ErrorVariable`, caught in script, or written by a
  command run through `invoke` reads the same way, so a cmdlet can
  sort or retry on what failed rather than on the text of it. It is
  `FromPs` only: a cmdlet raises an error by returning `Err(PsError)`
  and the engine builds the record. `ErrorCategory::from_code(u32)`
  maps the engine's number back to the enum, and a number outside it
  is an `InvalidData` error rather than a silent `NotSpecified`.

- `Pipeline::host_ui` hands back the cmdlet's `$Host.UI` as `HostUi`,
  with `read_line`, `read_line_as_secure_string`, `write_line` and
  `prompt_for_choice(caption, message, &[(label, help)], default)`
  typed rather than reached by dynamic incantation. Every call is a
  dynamic member access on the pipeline thread; no vtable entry was
  added. A host that cannot prompt, such as one run with
  `-NonInteractive`, refuses with the engine's own error, and that is
  the `Err`. The engine's method binder takes an array of
  `ChoiceDescription`s as the collection `PromptForChoice` declares,
  as it does for a script. The console host answers `read_line` and
  `prompt_for_choice` from redirected standard input but not
  `read_line_as_secure_string`, which reads the console device.

- `Pipeline::invoke` runs a cmdlet, function or alias the session can
  see, with parameters bound by name from a slice of pairs, and
  returns what it wrote; `invoke_with_input` pipes a value in first,
  unrolling a collection. The name is resolved to its `CommandInfo`
  and run through a nested `PowerShell` in the current runspace, so
  no script text is built or parsed. The command's non-terminating
  errors are written to the calling cmdlet's error stream and its
  output is returned, so the caller's `-ErrorAction` decides what a
  failure inside means; a terminating error is the `Err`. Vtable
  entry 70, pipeline thread.

- A `#[psmethods]` fn with no receiver is a static method of the
  generated proxy class, called on the type as `[Ns.Type]::Name()`,
  and the static named `new` returning `PsResult<Self>` is the
  class's constructor, reached as `[Ns.Type]::new(...)`. A static
  that returns the class returns a new proxy object. The shell
  reaches both through `PwrsCallStatic`, a `pwrs_proxy_call` with a
  null instance that serializes nothing and checks no generation,
  since there is no object to guard; the constructor's call answers
  the new value's pointer for the public constructor to adopt. The
  descriptor carries `static` and `constructor` per method. A type
  therefore needs no `New-` cmdlet to come into being.

- `#[on_import]` and `#[on_remove]` mark a `fn() -> PsResult<()>` the
  engine runs when the module is imported and when it is removed,
  named under `on_import` and `on_remove` in `export_module!`. The
  generated shell implements `IModuleAssemblyInitializer` and
  `IModuleAssemblyCleanup` for the hooks declared and no others, so a
  module without them is not called. A removal unloads nothing, so
  the import hook runs on every import and the removal hook is where
  an import's resources are released; on a reload the old library's
  removal hook runs before the new library's import hook. The export
  is `pwrs_module_lifecycle`, bound optionally, so a library built
  before it existed loads as before.

- Scalars keep their CLR width. `5i32.into_ps()` reaches the shell as
  a `System.Int32` rather than a `System.Int64`, and the same for
  `i8`, `i16`, `u8`, `u16`, `u32` and `f32`. The engine types every
  operator's answer by its operands' widths, so a value that left
  script as an `Int32` and returned as an `Int64` changed what the
  caller's next operator did with it. Vtable entries 59 to 65.

  A width costs one handle. `i64`, `f64` and `bool` are what the
  direct write entries build and still write without one; the other
  widths now take a handle per written value rather than widening.

- `PsObject::type_tag()` answers the tag of an object's own type in
  one crossing, against three and a string compare for `type_name()`,
  which is `GetType` then `FullName` then a string read. A caller
  dispatching on a type it does not know at compile time matches a
  `u32` and falls back to the name only for `PS_TYPE_OBJECT`. It
  unwraps a `PSObject` first, so a wrapped `Int32` answers
  `PS_TYPE_I32`. Vtable entry 58. Measured at 12 to 13 ns against 937
  to 1046 ns for `type_name()` and 345 to 1203 ns for
  `GetType().FullName` in script; `docs/PERF.md` has the table.

- `PsDecimal` and `PsDecimalBits` carry a `System.Decimal`, and tag 17
  makes a `Decimal[]` allocatable and pinnable. The two types are the
  same four words in the two orders that exist: `PsDecimal` is
  `Decimal.GetBits` order and `PsDecimalBits` is memory order, which
  is what a pinned element is. Scale is carried rather than
  normalized, since `1.10` and `1.1` are equal and not identical.

  Memory order is internal to the runtime. It was measured as
  `flags, hi, lo, mid` on x64 Windows under .NET 10 and .NET
  Framework 4.8, by two independent methods, and no reading exists
  for arm64, Linux or macOS. The first `pin::<PsDecimalBits>()` in a
  process therefore proves the order against `Decimal.GetBits` and
  caches the verdict; a host that disagrees fails with
  `PwrsDecimalLayout` instead of returning reinterpreted words.
  Vtable entries 66 and 67.

- `PsDateTimeOffset` carries a `System.DateTimeOffset`: ticks and the
  UTC offset in whole minutes. `PsDateTime`'s kind says only which
  clock a value belongs to, so an offset is what keeps an instant
  meaningful away from the host that produced it. Vtable entries 68
  and 69.

### Documentation

- How To Fix A Failure, keyed by the literal text a reader sees, each
  string taken from the source that raises it or measured on both
  hosts: the import that refuses with status 4, the descriptor ABI
  refusal, a native library missing for the platform, a missing
  export, pwsh not found, the toolchain directory, a rebuild that
  seems not to take, an error or a panic from a cmdlet, a prompt in a
  host that cannot ask, and `PWRS_MODULE` unset.

- That the crate and `cargo-pwrs` have to be the same version, in the
  quick start where the dependency is first written and leading the
  page above. It is the likeliest way to a module that will not
  import, and nothing said it.

- `pwrs-sys`, `pwrs-macros`, `pwrs-build` and `cargo-pwrs` each carry
  a README, so their pages say what the crate is and which of them a
  reader actually wants; each also carries keywords and categories.
  `PoWerRuSt` keeps the repository README.

- The locked-decisions list runs to forty-seven, having stopped at
  the conversion surface while eight more were settled.

- `Cmdlet` is the trait you implement and `CmdletMeta` and
  `CmdletBind` are what the macros generate, in the pipeline
  reference. All three are exported and only one is yours to write.

- Where the two hosts disagree, in the conversions reference. A
  string cast to `decimal` normalizes the scale on PowerShell 7 and
  keeps it on Windows PowerShell 5.1, so `[decimal]'1.10'` is scale 1
  on one and 2 on the other, while `[decimal]::Parse('1.10')` and the
  `1.10d` literal are 2 on both. And `-eq`, `-lt` and `-like` compare
  with the invariant culture, which is ICU on 7 and NLS on 5.1: those
  disagree on punctuation and ligatures and treat control characters
  as nothing, so text compared in Rust with ordinal semantics can
  reach a different answer than the same text compared in script, and
  the difference moves between hosts. The collation finding is a
  consumer's, attributed and not reproduced here.

### Fixed

- `PsObject::pin::<T>()` checked only that the array's elements were
  the same width as `T`, so an `Int64[]` pinned as `f64` and handed
  back the bits read as the wrong type, raising nothing. It now checks
  the element type as well and refuses with `PwrsPinElementType`,
  naming both tags. `Vec<T>` reads were never affected: that path
  compared the tag before pinning, and it keeps its single check
  through an internal entry rather than asking twice.

  The bound on `pin` gains `IntoPs`, which carries the type tag. Every
  primitive that could be pinned already satisfies it.

- Host vtable entry 57, `readonly_table_new`, appended with the size
  header covering it. No existing entry changes meaning. The type is
  constructed through it rather than by name because the engine's
  resolver does not see a type in the module's own load context, and
  a name would resolve on the desktop edition and not on Core.

## 0.1.7 - 2026-09-21

### Added

- `PWRS_TOOLSET` names the `Microsoft.Net.Compilers.Toolset` version to
  fetch, instead of the pinned one. The compiler runs inside the pwsh
  process, so it needs a runtime that host already has; a host whose
  newest pwsh predates .NET 10 sets this to 5.3.0, the newest toolset
  built for net9.0. Each version gets its own tree under
  `toolchain/`, so switching neither refetches nor mixes two compilers
  under one lock. Unset behaves exactly as before.

### Fixed

- A compiler whose runtime the host does not have was reported as
  `csc.dll has no entry point`, which is what the loader says when a
  framework is missing and reads as a damaged download. The message
  now names the framework the compiler wants, the one the host has,
  and `PWRS_TOOLSET`.

### Documentation

- macOS arm64 and FreeBSD x64 are recorded in `docs/PLATFORMS.md`,
  measured by a consumer on their own hosts and attributed as such.
  macOS needs nothing. FreeBSD needs `PWRS_TOOLSET=5.3.0` to build,
  and one variable set for Pester, whose `GetPesterOs` reads
  `$IsLinux` through `Get-Variable` and so is satisfied by a global.
- The Core half is no longer described as .NET 10. `net10.0` is where
  the `.psm1` looks, not what the compiler targets: that compile is
  `/nostdlib+` against the reference set the building pwsh ships and
  emits no `TargetFrameworkAttribute`, so the assembly references that
  host. The README's "No .NET 8 or 9" was refuted by a module built on
  pwsh 7.5.5 and imported on .NET 9.

## 0.1.6 - 2026-09-20

### Fixed

- `cargo pwrs new` wrote a fixed `PoWerRuSt` version into the manifest
  it scaffolds, and the README told the reader to depend on the same
  fixed version. Both had been left at 0.1.4 through the 0.1.5
  release. The scaffold now takes the version from `cargo-pwrs`
  itself, which the workspace publishes alongside the library, and a
  test fails the build when the README names any other version. The
  version requirement is a caret range, so a crate scaffolded by 0.1.5
  resolved to 0.1.5 regardless.
- `tools/hot_reload_gate.ps1` matched source lines with patterns
  anchored on `$`, which in multiline mode matches before the newline
  and not before a carriage return, so the gate failed on a checkout
  with CRLF endings. The patterns end in a lookahead now, and
  `.gitattributes` gives every platform the same bytes, which the
  shell assembly's name also depends on.

## 0.1.5 - 2026-09-20

### Added

- A rebuilt module takes its cmdlets over in a live session, in
  PowerShell 7 on Windows and Linux and in Windows PowerShell 5.1.
  `Import-Module -Force` after `cargo pwrs build` runs the code that
  was just built.
- `Pipeline::par_map` and `Pipeline::par_for_each` run owned `Send`
  work on a pool and write from the pipeline thread. `Order::Input`
  keeps input order, `Order::AsReady` writes each result as its worker
  finishes. The pool is std threads; the `parallel` feature swaps in
  Flynnel. `Order` is in the prelude.
- `tools/hot_reload_gate.ps1`, which builds one module three times in
  one host process and checks that a changed surface takes the cmdlets
  over and a changed body does not disturb the types. It passes in
  pwsh 7.6 and Windows PowerShell 5.1 on Windows, and in pwsh 7.6 on
  Linux.

Nothing is unloaded by a reload, and what that costs and when not to
rely on it are in the wiki's
[How To Reload A Module](https://github.com/Variably-Constant/PWRS/blob/main/wiki/content/docs/how-to/How-To-Reload-A-Module.md).

### Changed

- The shell assembly is named `<Module>.Shell.<stamp>.dll`, where the
  stamp is a hash of the managed source it was compiled from. A module
  folder's layout changes accordingly, including the help file name.
- Every load, managed and native, is taken from a copy under the
  process's own temporary folder. `cargo pwrs build` now succeeds while
  a session holds the module, and nothing is written inside an
  installed module folder.

### Fixed

- A worker that panicked under `par_map` dropped the items it held and
  the call returned success with a short stream. It now fails with a
  terminating `PwrsWorkerPanic`, which is what the documentation had
  always said.

## 0.1.4 - 2026-09-19

- The generated module manifest carries the properties a build can
  supply, each read from Cargo metadata and each covered by a test that
  has PowerShell parse the manifest and read the value back.

0.1.3 was never published; the version number was taken by another
piece of work and skipped.

## 0.1.2 - 2026-09-17

- Windows CI. A pwsh 7 process passes its `PSModulePath` to a Windows
  PowerShell 5.1 child, whose standard library is then unusable and
  whose errors name none of that. `cargo pwrs test` now gives the 5.1
  child a `PSModulePath` with the PowerShell 7 entries removed.
- The `netstandard2.0` compile takes a `System.Numerics.Vectors`
  reference, without which the vector paths raise CS0246.

## 0.1.1 - 2026-09-17

- The crates.io README. Relative links in a README resolve against the
  crate's own directory rather than the repository root, so the links
  in 0.1.0 were broken; they are absolute now. crates.io freezes a
  README per version, which is why this needed a release.

## 0.1.0 - 2026-09-17 [YANKED]

First publish. Yanked: it shipped a README from before the crates were
on crates.io, which a published version cannot be corrected in place.
