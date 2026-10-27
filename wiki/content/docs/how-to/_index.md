---
title: How-to
weight: 2
sidebar:
  open: true
---

Task-oriented recipes. Each page solves one concrete problem and assumes the basics from [Tutorials](../tutorials/).

{{< cards >}}
  {{< card link="how-to-write-a-cmdlet/" title="How To Write A Cmdlet" subtitle="Parameters, phases, output, streams, errors, ShouldProcess, prompts, cancellation." >}}
  {{< card link="how-to-return-objects/" title="How To Return Objects" subtitle="Copied, proxy and psobject classes, enums, default views, and when to pick which." >}}
  {{< card link="how-to-write-a-provider/" title="How To Write A Provider" subtitle="The Provider trait mapped onto NavigationCmdletProvider, with an in-memory filesystem." >}}
  {{< card link="how-to-add-completers-and-dynamic-parameters/" title="How To Add Completers, Transforms And Dynamic Parameters" subtitle="Tab completion, arguments changed before binding, and parameters that depend on others." >}}
  {{< card link="how-to-call-dotnet-from-rust/" title="How To Call .NET From Rust" subtitle="Properties, methods, statics, constructors, script blocks, commands, and zero-copy arrays." >}}
  {{< card link="how-to-use-threads/" title="How To Use Threads" subtitle="The pipeline-thread rule, stream_from_thread, workers, par_map, and cancellation." >}}
  {{< card link="how-to-test-a-module/" title="How To Test A Module" subtitle="Pester in both hosts, the fake host for Rust unit tests, and the in-process engine." >}}
  {{< card link="how-to-add-hybrid-csharp/" title="How To Add Hybrid C#" subtitle="Hand-written C# cmdlets in the shell assembly, and the desktop assemblies they can use." >}}
  {{< card link="how-to-publish/" title="How To Publish" subtitle="cargo pwrs publish, the dry run, the gallery key, and building on several platforms." >}}
  {{< card link="how-to-sign-a-module/" title="How To Sign A Module" subtitle="Authenticode signatures on scripts and executables, on any platform, with any key." >}}
  {{< card link="how-to-make-a-module-fast/" title="How To Make A Module Fast" subtitle="Pipeline input, the phase mask, bulk data through a pin, and what each is worth." >}}
  {{< card link="how-to-use-instruction-sets/" title="How To Use Instruction Sets" subtitle="AVX2, AVX-512 and the rest chosen at run time, and every tier tested on one machine." >}}
  {{< card link="how-to-pass-native-data/" title="How To Pass Native Data Between Cmdlets" subtitle="One object per stage taken by type, its gate, and what the rows cost." >}}
  {{< card link="how-to-ship-a-helper-executable/" title="How To Ship A Helper Executable" subtitle="A [[bin]] target built with the library, shipped beside it, started from a staged copy." >}}
  {{< card link="how-to-ship-a-prebuilt-native-file/" title="How To Ship A Prebuilt Native File" subtitle="A file from a pinned download, shipped beside the library, its licenses in the notices." >}}
  {{< card link="how-to-bundle-a-module/" title="How To Bundle A Module" subtitle="Another PWRS module shipped inside this one, imported with it, and published with it." >}}
  {{< card link="how-to-raise-engine-events/" title="How To Raise Engine Events" subtitle="Engine events raised from any Rust thread, during the call or after it returns." >}}
  {{< card link="how-to-trace-a-module/" title="How To Trace A Module" subtitle="PWRS_TRACE, the counters on both sides, and how to read a line." >}}
  {{< card link="how-to-reload-a-module/" title="How To Reload A Module" subtitle="Picking up a rebuild in a live session, and what stays loaded until the process exits." >}}
  {{< card link="how-to-run-powershell-in-a-rust-program/" title="How To Run PowerShell In A Rust Program" subtitle="pwrs-host: find the installed pwsh, start its runtime in the process, call an assembly." >}}
  {{< card link="how-to-use-runspaces-jobs-and-remote-sessions/" title="How To Use A Module In Runspaces, Jobs And Remote Sessions" subtitle="Imported in each runspace, objects shared in a process and copied between processes." >}}
  {{< card link="how-to-debug-a-module/" title="How To Debug A Module" subtitle="Breakpoints in Rust code with WinDbg, gdb and lldb, attaching to a shell, and panics." >}}
  {{< card link="how-to-fix-a-failure/" title="How To Fix A Failure" subtitle="What each failure means, keyed by the text you see." >}}
{{< /cards >}}
