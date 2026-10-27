---
title: Tutorials
weight: 1
sidebar:
  open: true
---

One module, FileFacts, built from a scaffold to a package for the PowerShell Gallery in nine chapters, read in order. Each chapter adds to the same Rust crate and ends with every file it changed, and every example on these pages was run as printed, in PowerShell 7 and in Windows PowerShell 5.1.

{{< cards >}}
  {{< card link="getting-started/" title="1. Getting Started" subtitle="Install cargo pwrs, make the module, build it, and run its first command in both hosts." >}}
  {{< card link="parameters-and-the-pipeline/" title="2. Parameters and the Pipeline" subtitle="Paths with wildcards and without, values from the pipeline, and parameter sets." >}}
  {{< card link="talking-to-the-user/" title="3. Talking to the User" subtitle="Errors, warnings, verbose text, progress, console colors, and -WhatIf and -Confirm." >}}
  {{< card link="objects-out/" title="4. Objects Out" subtitle="The module's own classes, how a table shows them, an enum, and an object stored in Rust." >}}
  {{< card link="objects-in/" title="5. Objects In" subtitle="Script blocks, hashtables and .NET objects passed to the module." >}}
  {{< card link="help-and-the-binder/" title="6. Help and the Binder" subtitle="Help from doc comments, an alias, Tab completion, a dynamic parameter, and a transform." >}}
  {{< card link="long-work/" title="7. Long Work" subtitle="Work spread over every processor, progress from workers, Ctrl+C, and an engine event." >}}
  {{< card link="testing/" title="8. Testing" subtitle="Rust unit tests, Pester in both hosts through cargo pwrs test, and what no test exercised." >}}
  {{< card link="shipping/" title="9. Shipping" subtitle="The author, the release build, license notices, a dry run, and publishing." >}}
{{< /cards >}}

The [Hello Tour](../reference/hello-tour/) goes through the repository's own example module, command by command, as a reference for each mechanism.
