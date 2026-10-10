---
title: Explanation
weight: 3
sidebar:
  open: true
---

Discussion that clarifies and illuminates. The "why" behind the code: the problem, the design choices, the costs, and the comparison with the project PWRS is modeled on.

{{< cards >}}
  {{< card link="why-pwrs/" title="Why PWRS" subtitle="The problem PyO3 does not have, the four mechanisms considered, and the one chosen." >}}
  {{< card link="the-bridge/" title="The Bridge" subtitle="The generated shell, the append-only vtable, the native exports, and the two rules that hold at the boundary." >}}
  {{< card link="the-call-path/" title="The Call Path" subtitle="What one invocation costs, piece by piece, and how each piece was measured." >}}
  {{< card link="two-hosts/" title="Two Hosts" subtitle="How one build serves PowerShell 7 from 7.4 on and Windows PowerShell 5.1 on .NET Framework." >}}
  {{< card link="pwrs-and-pyo3/" title="PWRS and PyO3" subtitle="Concept by concept: what carried over, what had to change, what each has that the other lacks." >}}
{{< /cards >}}
