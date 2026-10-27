---
title: How To Use A Module In Runspaces, Jobs And Remote Sessions
weight: 20
---

What a PWRS module and its objects do when a script uses other runspaces, background jobs or remote sessions. Source: `crates/cargo-pwrs/dotnet/Pwrs.Runtime/ProxyCore.cs` (the gate that runs a proxy's calls one at a time), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/NativeModule.cs` (the native library a process loads for a module), `crates/cargo-pwrs/dotnet/Pwrs.Bootstrap/Loader.cs` (each process's staged copy), and the hello example's `tests/Runspaces.Tests.ps1`.

A runspace is one PowerShell session inside a process, with its own variables, modules and commands. `ForEach-Object -Parallel`, `Start-ThreadJob` and a `RunspacePool` run scripts in other runspaces of the same process. `Start-Job` runs a script in a new process, and `Invoke-Command` runs one in a remote session: on another machine, or in another process.

Everything on this page was run with the hello example in pwsh 7.6.7 and Windows PowerShell 5.1 on Windows, and over SSH to pwsh 7.6.6 on Ubuntu 24.04. `Runspaces.Tests.ps1` checks the runspace and job results in both hosts. In the examples, `$manifest` is the full path of the module's `Hello.psd1`.

## Each runspace imports the module

A new runspace has none of the module's commands, even when the runspace that made it has the module. In a runspace made with `[runspacefactory]::CreateRunspace()`, `Get-Person` fails with `CommandNotFoundException` until that runspace imports the module. There are three ways to give a runspace the module.

Import it by path in the runspace:

```powershell
1..4 | ForEach-Object -Parallel {
    Import-Module $using:manifest
    Get-Person "p$_"
}
```

Or put the folder the module's folder is in on `PSModulePath`. A runspace then loads the module the first time a script there calls one of its commands. hello's module folder is `target\pwrs\Hello`, so this lets every runspace of the process find `Get-Person`:

```powershell
$env:PSModulePath = (Resolve-Path 'target\pwrs').Path + [IO.Path]::PathSeparator + $env:PSModulePath
```

Or give a `RunspacePool` an `InitialSessionState` that imports the module, so every runspace the pool opens has it:

```powershell
$iss = [System.Management.Automation.Runspaces.InitialSessionState]::CreateDefault()
$iss.ImportPSModule($manifest)
$pool = [System.Management.Automation.Runspaces.RunspaceFactory]::CreateRunspacePool(1, 4, $iss, $Host)
$pool.Open()
```

The pool imports the module into each runspace as it opens it. That also avoids a failure inside PowerShell when runspaces of one pool import a module at the same moment; [How To Fix A Failure](How-To-Fix-A-Failure.md#import-fails) has its message.

Every runspace of the process uses the one native library the first import loaded. A module that makes something for the whole process makes it in its load hook, which runs at the first of those imports. It releases it in its unload hook, which runs once no runspace has the module. A runspace that closes without `Remove-Module` counts as removing it. The runspaces of a `ForEach-Object -Parallel` run, a thread job and a pool close when those end. [Attribute Reference](../reference/Attribute-Reference.md) describes the four lifecycle hooks.

## Objects within one process

Runspaces of one process pass each other the object itself, not a copy. `$using:` in `ForEach-Object -Parallel` and `Start-ThreadJob` does this, and so does `SessionStateProxy.SetVariable` on a runspace you make. This is true for each kind of class a module can have, which [How To Return Objects](How-To-Return-Objects.md) describes:

- a copied class's object, such as a `Hello.Person`, is a .NET object with the values in its properties;
- a proxy class's object, such as a `Hello.Counter`, has its value stored in Rust, so every runspace that has the object reads and changes that one value;
- a psobject class's object, such as a `Hello.Note`, is a `PSObject` with note properties.

A proxy's calls run one at a time, whichever runspace makes them, so a method that changes the value never runs beside another call on the same object. Eight runspaces each calling `Advance(1)` a thousand times on one counter leave it at 8000:

```powershell
$c = New-Counter shared
1..8 | ForEach-Object -ThrottleLimit 8 -Parallel {
    $c = $using:c
    foreach ($n in 1..1000) { $null = $c.Advance(1) }
}
$c.Value   # 8000
```

Four runspaces made with `CreateRunspace`, each calling it 2000 times at once, leave 8000 values in the counter's `History`, every one different, in both hosts.

What a runspace returns comes back as it is. A counter made in `ForEach-Object -Parallel` or in a thread job still has its methods in the runspace it is returned to:

```powershell
$c = Start-ThreadJob { Import-Module $using:manifest; New-Counter made -Value 3 } | Receive-Job -Wait -AutoRemoveJob
$c.Advance(1)   # 4
```

`Start-ThreadJob` is in the `Microsoft.PowerShell.ThreadJob` module, which pwsh 7.6.7 on Windows and pwsh 7.6.6 on Linux include. pwsh 7.5.5 for FreeBSD has no such module, and the Windows PowerShell 5.1 measured here had no such command.

A rebuild reaches every runspace. A process uses one copy of a module's native library at a time, so `Import-Module -Force` on a rebuilt module in one runspace moves every runspace of the process to the new copy. A proxy object made before that refuses its next call with `this Counter was made by load 0 of the module and load 1 is running; make it again`. [How To Reload A Module](How-To-Reload-A-Module.md) covers reloads.

## Jobs and remote sessions

`Start-Job` runs its script in a new PowerShell process, and `Invoke-Command` runs its script in a remote session. Each imports the module on its own, by path or from its own `PSModulePath`, so a remote machine needs the module installed. Without that import, `Get-Person` fails there with `CommandNotFoundException`.

Objects cross between processes as copies of their properties. PowerShell writes each object's properties out, and the other side builds a new object from them, a deserialized object, whose type name starts with `Deserialized.`:

```powershell
$person, $counter = Start-Job {
    Import-Module $using:manifest
    Get-Person Ann -Age 30
    New-Counter j -Value 5
} | Receive-Job -Wait -AutoRemoveJob
$person.PSObject.TypeNames[0]    # Deserialized.Hello.Person
$person.Age                      # 30
$counter.PSObject.TypeNames[0]   # Deserialized.Hello.Counter
$counter.Value                   # 5
```

A deserialized object has the properties and none of the methods, so `$counter.Advance(1)` fails with `MethodNotFound`. A proxy's value stays in the process that made it.

Three things still work:

- A copied object that came back binds to a parameter of its class. `New-RustTeam -Lead $person` takes the person above and returns a team whose lead is a `Hello.Person` named Ann.
- A proxy object that came back does not bind: `Get-RustCounterText -Counter $counter` is refused with `CannotConvertArgumentNoMessage`. A class with a constructor makes it again from its properties, as `[Hello.Counter]::new($counter.Label, $counter.Value)` does.
- A proxy stored inside a remote session goes on working there. A session's variables stay between calls, so a counter stored in one is the same live counter at the next `Invoke-Command`:

```powershell
$s = New-PSSession -HostName server -UserName me
Invoke-Command -Session $s { Import-Module Hello; $global:c = New-Counter kept -Value 1 }
Invoke-Command -Session $s { $global:c.Advance(5) }   # 6
Remove-PSSession $s
```

Each of these transports gave those same results:

- `Start-Job`, in both hosts. From pwsh, `Start-Job -PSVersion 5.1` runs the job in Windows PowerShell, where the module works too.
- `Invoke-Command -ComputerName` over WinRM, run as `Invoke-Command -ComputerName localhost -EnableNetworkAccess` from both hosts to Windows PowerShell 5.1 on the same machine.
- `Invoke-Command -HostName` over SSH, from pwsh to pwsh 7.6.6 on Ubuntu and to pwsh 7.6.7 on the same Windows machine. Windows PowerShell 5.1's `Invoke-Command` has no `-HostName`.
- `New-PSSession -UseWindowsPowerShell` from pwsh, a session in a Windows PowerShell 5.1 process.

`Import-Module -PSSession $s -Name Hello` makes local commands that run the module's commands in the session, and they return deserialized objects too. `Import-Module -UseWindowsPowerShell` in pwsh works the same way, through a Windows PowerShell 5.1 session.

## Each process has its own staged copy

PWRS loads a module's native library and .NET files from a copy it makes under the temporary folder, in `pwrs-load\<process id>`, as [Two Hosts](../explanation/Two-Hosts.md) explains. A job's process makes a copy of its own. When a process has ended, the next process to import a PWRS module deletes its folder. After a job's process ended, its folder was still there; after a second job imported the module, the first job's folder was gone, in both hosts.
