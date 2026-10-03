# PWRS platforms

The Rust code and the managed shells are platform-neutral; only the
native cdylib under `runtimes/<rid>/native/` differs per platform, and
it must be linked with the target's own linker. So each platform builds
on its own machine.

## Status

Every row runs this repository's own suites on the tree as released:
the workspace's Rust tests, `cargo pwrs test` for each example with
its surface check, the hot reload gate and the co-load gate. Windows,
Linux and FreeBSD run on machines of the project's own with rustc
1.98.1, where the Rust tests and hello's suite run again with the
`parallel` feature,
clippy runs with warnings denied with the default features and with
all of them, and the lock is checked to be the one the tree carries.
Windows adds the wire audit, the PowerShell Gallery's analyzer rules
over each example's script and manifest, and the packaging of the five
crates. macOS runs on a hosted runner through
`.github/workflows/ci.yml` at each push to main.

| Platform | Machine | pwsh 7 | Windows PowerShell 5.1 |
|---|---|---|---|
| Windows x64 | Windows 11 Pro 10.0.26200, Ryzen 9 7900X | every suite in pwsh 7.6.6 on .NET 10.0.12 | every suite in 5.1.26100.9444 |
| Linux x64 | Ubuntu 24.04 VM, Linux 6.8, glibc 2.39 | every suite in pwsh 7.6.6 on .NET 10.0.12 from Microsoft's tarball, and hello again in 7.4.20 and 7.5.11 from the same build; every suite built for glibc 2.35 in the snap's pwsh 7.6.5 on .NET 10.0.11 | not applicable |
| FreeBSD x64 | FreeBSD 15.0 VM | every suite in pwsh 7.5.5 on .NET 9.0.14 | not applicable |
| macOS arm64 | macos-latest hosted runner, image macos-26-arm64 | every suite in pwsh 7.6.5 on .NET 10.0.11 | not applicable |

"Every suite" is hello's 380 tests, memfs's 12 and tls's 4, one of
which needs the network; calc has no suite of its own, and hello's
imports it as its bundled module.

## PowerShell 7 versions

A module's PowerShell 7 half is compiled against .NET 8's reference
pack and the `System.Management.Automation` 7.4.0 reference from NuGet,
whichever machine builds it, so it references `System.Runtime` 8.0.0.0
and `System.Management.Automation` 7.4.0.0 from every machine, and its
script refuses a PowerShell 7 on a .NET older than 8 by naming the
PowerShell it needs.

hello built on the Ubuntu VM by pwsh 7.6.6 passes all 380 tests under
pwsh 7.4.20 on .NET 8.0.31, 7.5.11 on .NET 9.0.20 and 7.6.6 on .NET
10.0.12, the first two from Microsoft's release tarballs, run through
the same `pester.ps1` `cargo pwrs test` uses. A module built by
`cargo-pwrs` 0.2.0 or earlier does not have this property; see
[How To Fix A Failure](https://variably-constant.github.io/PWRS/docs/how-to/how-to-fix-a-failure/).

## Linux

The Ubuntu 24.04 VM runs cargo 1.98.1 and carries two installs of
pwsh: the powershell snap, pwsh 7.6.5 on .NET 10.0.11, which is the
one on `PATH`, and Microsoft's tarball, pwsh 7.6.6 on .NET 10.0.12.
Every suite passes under the tarball's. Under the snap's, every suite
passes built for glibc 2.35, and two examples built for the host's own
glibc do not load, below.

The snap runs pwsh on its base's libraries rather than the host's:
glibc 2.35 from core22, and an ICU 70.1 of its own. Two things follow.
The workspace's in-process test host, which starts .NET from the pwsh
on `PATH`, has to load the snap's ICU itself, and does; see
[How To Test A Module](https://variably-constant.github.io/PWRS/docs/how-to/how-to-test-a-module/).
And a module whose native library needs a newer glibc symbol than the
snap's base provides does not load under the snap's pwsh, while it
loads under the tarball's; see
[How To Fix A Failure](https://variably-constant.github.io/PWRS/docs/how-to/how-to-fix-a-failure/).
Built on this VM, whose glibc is 2.39, two examples are such modules.
`examples/tls`'s library needs `GLIBC_2.38` for `__isoc23_sscanf` and
`__isoc23_strtol`, which aws-lc's C code calls. `examples/hello`'s needs
`GLIBC_2.39` for `pidfd_spawnp` and `pidfd_getpid`, which the Rust
standard library links for starting a process, as hello's helper
cmdlets do. calc's and memfs's need nothing past `GLIBC_2.34` and load
under both. Built with `--target x86_64-unknown-linux-gnu.2.35`, which
`cargo pwrs` hands to cargo-zigbuild, the tls and hello libraries and
hello's helper need nothing past `GLIBC_2.34` either, and every suite
and both gates pass under the snap's pwsh; the steps are in
[How To Fix A Failure](https://variably-constant.github.io/PWRS/docs/how-to/how-to-fix-a-failure/).

## FreeBSD

The FreeBSD 15.0 VM runs pwsh 7.5.5 on .NET 9.0.14; FreeBSD's packaged
.NET tops out at 9. The gates build with rustc and cargo 1.98.1 from a
rustup of their own, since the workspace needs 1.98. Two settings make
everything pass.

The compile needs a toolset its .NET can run. The pinned compiler
toolset is built for net10.0, so the compile has no runtime to start
on; `PWRS_TOOLSET=5.3.0` names the newest toolset built for net9.0 and
the build completes. What it compiles against does not depend on that
pwsh: the PowerShell 7 half references .NET 8's reference pack and the
`System.Management.Automation` 7.4.0 reference from NuGet on FreeBSD as
everywhere else, so a module built there loads where one built on any
other machine does. See [PowerShell 7 versions](#powershell-7-versions).

Pester needs one variable. `Invoke-Pester` throws "Unsupported
Operating system!" from its own `GetPesterOs`, because pwsh on FreeBSD
reports `IsWindows`, `IsLinux` and `IsMacOS` all false and the
function has no branch left. It reads them with `Get-Variable`, which
resolves through the scope chain rather than reading the automatic
variable, so a global of the same name answers it. `cargo pwrs test`
imports whatever `PWRS_PESTER_PATH` names in place of Pester, so
pointing it at a module that sets the global and then imports Pester
lets the whole command run unchanged, which is how the FreeBSD row is
measured:

```powershell
# PesterIsLinuxShim.psm1, in the folder Save-Module wrote Pester 6.2.0 into
Set-Variable -Name IsLinux -Value $true -Scope Global -Force
Import-Module (Join-Path $PSScriptRoot 'Pester/6.2.0/Pester.psd1') -Global -ErrorAction Stop
```

`cargo pwrs test` does not set the variable itself, and
`crates/cargo-pwrs/scripts/pester.ps1` does not either, because claiming to be Linux on
a user's behalf is not a decision this tool should take quietly. No
test in this repository reads the platform variables.

Two things that look like the workaround and are not. Pester 4.10.1
has no OS check and so runs unaided, then fails everything written in
Pester 5 syntax. And a hand-rolled `Invoke-Pester` that omits
`PWRS_MODULE` reports every test as a silent failure with a null error
record, because a failed `BeforeAll` is counted once per test; the
real message is on the container, at
`$r.Containers[0].ErrorRecord[0]`.

## macOS

The project owns no Mac, so the macOS row comes from
`.github/workflows/ci.yml`'s `macos-arm64` job on a hosted runner,
which saves Pester 6.2.0 and runs the Rust tests, every example's
suite and both gates in the runner's own pwsh at each push to main.
The native library builds as `runtimes/osx-arm64/native/*.dylib` and
needs no setting. On this tree the runner's image was macos-26-arm64,
with pwsh 7.6.5 on .NET 10.0.11 and rustc 1.99.0, and every suite
passed under Pester 6.2.0: hello's 380 tests, memfs's 12 and tls's 4,
with the reload and co-load gates.

## TLS from a module on Windows

`examples/tls` makes its own HTTPS connections through rustls, whose
default provider offers X25519MLKEM768 first, while PowerShell's
`Invoke-WebRequest` goes through the Windows TLS stack, SChannel. What
each negotiated with `https://pq.cloudflareresearch.com/cdn-cgi/trace`,
read from the server's own trace:

| Windows | Host | `Invoke-WebRequest` | The module |
|---|---|---|---|
| 11 Pro 10.0.26200 | pwsh 7.6.6 on .NET 10.0.12 | TLS 1.3, X25519 | TLS 1.3, X25519MLKEM768, TLS13_AES_256_GCM_SHA384 |
| 11 Pro 10.0.26200 | Windows PowerShell 5.1.26100.9444 on .NET Framework 4.8.9345 | TLS 1.3, X25519, and the same with TLS 1.3 alone requested | TLS 1.3, X25519MLKEM768, TLS13_AES_256_GCM_SHA384 |
| Server 2019 Standard Evaluation 10.0.17763 | Windows PowerShell 5.1.17763.2931 on .NET Framework release 461814 | TLS 1.2, X25519 | TLS 1.3, X25519MLKEM768, TLS13_AES_256_GCM_SHA384 |

On Server 2019, Windows PowerShell's `[Net.ServicePointManager]::SecurityProtocol`
was `Tls, Tls11, Tls12`, and TLS 1.3 could not be asked for: its value,
12288, was refused as not a `SecurityProtocolType`. The module's
connections do not go through that setting.

The module was built with
`RUSTFLAGS='-C target-feature=+crt-static -C target-cpu=x86-64'`,
which links the C runtime into the library: `dumpbin /dependents` lists
only `api-ms-win-core-synch-l1-2-0`, `bcryptprimitives`, `KERNEL32`,
`ntdll` and `WS2_32`, with no `VCRUNTIME140` and no `api-ms-win-crt`
set. The Server 2019 row ran on a fresh installation whose `System32`
had no `vcruntime140.dll` or `msvcp140.dll`, so no Visual C++
Redistributable was present, and the module's suite passed 4 of 4 there
in Windows PowerShell 5.1. That installation ran as a virtual machine
under QEMU 11.1.0 with software CPU emulation, on a Ryzen 9 7900X.

## Building the native library per platform

```
cargo pwrs build --release                                        # the building machine's rid
cargo pwrs build --release --target x86_64-unknown-linux-gnu      # a triple with a linker configured
cargo pwrs merge target/pwrs/<Module> <folder built on Linux> ... # one folder, every rid
```

`cargo pwrs build` places the library at `runtimes/<rid>/native/`
with the rid of the building machine or of the `--target` triple
(`win`/`linux`/`osx`/`freebsd` and `x64`/`arm64`/`x86`); the
`NativeModule` resolver reads the same layout at load. A `--target`
that is not the host's needs a cross linker in cargo's configuration,
and the build also compiles the crate for the host, whose library is
the one loaded to read the descriptor. `cargo pwrs merge` copies the
`runtimes/<rid>` folders of module folders built elsewhere into one,
refusing a rid already present and manifests that differ.

That path was carried end to end with a module of 135 cmdlets built
outside this repository. Its Windows and
Linux builds of one checkout produced byte-identical manifests, so
`merge` folded `runtimes/linux-x64` into the `win-x64` folder first
time, and the one folder then imported and ran in pwsh 7.6.6 and
Windows PowerShell 5.1 on Windows off the `.dll` and in pwsh 7.6.5 on
Ubuntu 24.04 off the `.so`. That module's own suite of 176 tests passed
in full on each of the three. It is evidence for `merge` and the rid
layout rather than for the per-platform rows above, which are this
repository's own suites.

## Hosted runners

`.github/workflows/ci.yml` builds and tests on `macos-latest` (arm64,
pwsh 7) only. Windows, Linux and FreeBSD run on the machines above,
and FreeBSD has no hosted runner.
