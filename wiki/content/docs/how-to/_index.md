---
title: How-to
weight: 2
sidebar:
  open: true
---

Task-oriented recipes. Each page solves one concrete problem and assumes the basics from [Tutorials](../tutorials/).

{{< cards >}}
  {{< card link="how-to-write-a-cmdlet/" title="How To Write A Cmdlet" subtitle="Parameters and their attributes, the three phases, output, errors, streams, ShouldProcess, cancellation." >}}
  {{< card link="how-to-return-objects/" title="How To Return Objects" subtitle="Copied, proxy and psobject classes, enums, arrays, hashtables, and when to pick which." >}}
  {{< card link="how-to-write-a-provider/" title="How To Write A Provider" subtitle="The Provider trait mapped onto NavigationCmdletProvider, with the in-memory filesystem as the worked example." >}}
  {{< card link="how-to-add-completers-and-dynamic-parameters/" title="How To Add Completers, Transforms And Dynamic Parameters" subtitle="Tab completion from Rust, arguments changed before the binder coerces them, and parameters that appear depending on what was bound." >}}
  {{< card link="how-to-call-dotnet-from-rust/" title="How To Call .NET From Rust" subtitle="Properties, methods, statics, constructors, script blocks, and zero-copy primitive arrays." >}}
  {{< card link="how-to-use-threads/" title="How To Use Threads" subtitle="stream_from_thread, par_map, the pipeline-thread rule, and cancellation." >}}
  {{< card link="how-to-test-a-module/" title="How To Test A Module" subtitle="Pester in both hosts through cargo pwrs test, the fake host for Rust unit tests, the in-process engine host." >}}
  {{< card link="how-to-add-hybrid-csharp/" title="How To Add Hybrid C#" subtitle="Hand-written C# cmdlets compiled into the same shell assembly." >}}
  {{< card link="how-to-publish/" title="How To Publish" subtitle="cargo pwrs publish, the dry run, and the gallery key." >}}
  {{< card link="how-to-make-a-module-fast/" title="How To Make A Module Fast" subtitle="Pipeline input against per-invocation calls, the phase mask, bulk data through a pin, and what each is worth." >}}
  {{< card link="how-to-use-instruction-sets/" title="How To Use Instruction Sets" subtitle="AVX2, AVX-512 and the rest on both hosts: run-time dispatch, the import check, and every tier tested on one machine." >}}
  {{< card link="how-to-pass-native-data/" title="How To Pass Native Data Between Cmdlets" subtitle="One object per stage taken by type, the object's gate, telling the collector what it holds, and what the rows cost." >}}
  {{< card link="how-to-ship-a-helper-executable/" title="How To Ship A Helper Executable" subtitle="A [[bin]] target built with the library, shipped beside it, and started from a copy staged for the process." >}}
  {{< card link="how-to-bundle-a-module/" title="How To Bundle A Module" subtitle="Another PWRS module built by the same tool, laid inside this one, imported with it, and carried through publish and merge." >}}
  {{< card link="how-to-raise-engine-events/" title="How To Raise Engine Events" subtitle="PsEvents from any thread, during the call and after it, read in script by Wait-Event, Get-Event and Register-EngineEvent -Action." >}}
  {{< card link="how-to-trace-a-module/" title="How To Trace A Module" subtitle="PWRS_TRACE, the counters on both sides, and how to read a line." >}}
  {{< card link="how-to-reload-a-module/" title="How To Reload A Module" subtitle="Picking up a rebuild in a live session, why nothing is unloaded, and what that costs." >}}
  {{< card link="how-to-fix-a-failure/" title="How To Fix A Failure" subtitle="What each failure means, keyed by the text you see; the crate and cargo-pwrs move together." >}}
{{< /cards >}}
