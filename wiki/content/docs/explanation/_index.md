---
title: Explanation
weight: 3
sidebar:
  open: true
---

How PWRS works underneath: the problem it solves, the bridge between the shell and Rust, what a call costs, the two hosts, and how it compares with PyO3.

{{< cards >}}
  {{< card link="why-pwrs/" title="Why PWRS" subtitle="The problem PyO3 does not have, and the shell, the bridge and the build that solve it." >}}
  {{< card link="the-bridge/" title="The Bridge" subtitle="The generated shell, the vtable, the native exports, and the two rules at the boundary." >}}
  {{< card link="the-call-path/" title="The Call Path" subtitle="What one invocation costs, piece by piece, and how each piece was measured." >}}
  {{< card link="two-hosts/" title="Two Hosts" subtitle="How one build runs in PowerShell 7 from 7.4 on and in Windows PowerShell 5.1." >}}
  {{< card link="pwrs-and-pyo3/" title="PWRS and PyO3" subtitle="What PWRS and PyO3 share, how they differ, and what each has that the other lacks." >}}
{{< /cards >}}
