---
title: How-to
weight: 2
sidebar:
  open: true
---

Task-oriented recipes. Each page solves one concrete problem and assumes the basics from [Tutorials](../tutorials/).

{{< cards >}}
  {{< card link="How-To-Write-A-Cmdlet/" title="How To Write A Cmdlet" subtitle="Parameters and their attributes, the three phases, output, errors, streams, ShouldProcess, cancellation." >}}
  {{< card link="How-To-Return-Objects/" title="How To Return Objects" subtitle="Copied, proxy and psobject classes, enums, arrays, hashtables, and when to pick which." >}}
  {{< card link="How-To-Write-A-Provider/" title="How To Write A Provider" subtitle="The Provider trait mapped onto NavigationCmdletProvider, with the in-memory filesystem as the worked example." >}}
  {{< card link="How-To-Add-Completers-And-Dynamic-Parameters/" title="How To Add Completers, Transforms And Dynamic Parameters" subtitle="Tab completion from Rust, arguments changed before the binder coerces them, and parameters that appear depending on what was bound." >}}
  {{< card link="How-To-Call-DotNet-From-Rust/" title="How To Call .NET From Rust" subtitle="Properties, methods, statics, constructors, script blocks, and zero-copy primitive arrays." >}}
  {{< card link="How-To-Use-Threads/" title="How To Use Threads" subtitle="stream_from_thread, par_map, the pipeline-thread rule, and cancellation." >}}
  {{< card link="How-To-Test-A-Module/" title="How To Test A Module" subtitle="Pester in both hosts through cargo pwrs test, the fake host for Rust unit tests, the in-process engine host." >}}
  {{< card link="How-To-Add-Hybrid-CSharp/" title="How To Add Hybrid C#" subtitle="Hand-written C# cmdlets compiled into the same shell assembly." >}}
  {{< card link="How-To-Publish/" title="How To Publish" subtitle="cargo pwrs publish, the dry run, and the gallery key." >}}
  {{< card link="How-To-Make-A-Module-Fast/" title="How To Make A Module Fast" subtitle="Pipeline input against per-invocation calls, the phase mask, bulk data through a pin, and what each is worth." >}}
  {{< card link="How-To-Use-Instruction-Sets/" title="How To Use Instruction Sets" subtitle="AVX2, AVX-512 and the rest on both hosts: run-time dispatch, the import check, and every tier tested on one machine." >}}
  {{< card link="How-To-Pass-Native-Data/" title="How To Pass Native Data Between Cmdlets" subtitle="One object per stage taken by type, the object's gate, telling the collector what it holds, and what the rows cost." >}}
  {{< card link="How-To-Ship-A-Helper-Executable/" title="How To Ship A Helper Executable" subtitle="A [[bin]] target built with the library, shipped beside it, and started from a copy staged for the process." >}}
  {{< card link="How-To-Bundle-A-Module/" title="How To Bundle A Module" subtitle="Another PWRS module built by the same tool, laid inside this one, imported with it, and carried through publish and merge." >}}
  {{< card link="How-To-Trace-A-Module/" title="How To Trace A Module" subtitle="PWRS_TRACE, the counters on both sides, and how to read a line." >}}
  {{< card link="How-To-Reload-A-Module/" title="How To Reload A Module" subtitle="Picking up a rebuild in a live session, why nothing is unloaded, and what that costs." >}}
  {{< card link="How-To-Fix-A-Failure/" title="How To Fix A Failure" subtitle="What each failure means, keyed by the text you see; the crate and cargo-pwrs move together." >}}
{{< /cards >}}
