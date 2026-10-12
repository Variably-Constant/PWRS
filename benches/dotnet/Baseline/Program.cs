using System.Management.Automation;
using System.Management.Automation.Runspaces;
using BenchmarkDotNet.Attributes;
using BenchmarkDotNet.Running;

// Times a hand-written C# cmdlet against the generated pwrs cmdlet
// doing the same work, through one real runspace. Run on a quiet box;
// BenchmarkDotNet handles warmup and iteration. Point PWRS_MODULE at a
// built Hello module folder so the pwrs cmdlet is importable.
//
// dotnet run -c Release -- --filter *

BenchmarkRunner.Run<GreetingBenchmarks>();

/// <summary>The C# baseline cmdlet: the same work Get-Greeting does.</summary>
[Cmdlet(VerbsCommon.Get, "GreetingBaseline")]
public sealed class GetGreetingBaselineCommand : PSCmdlet
{
    [Parameter(Mandatory = true, Position = 0, ValueFromPipeline = true)]
    public string Name { get; set; } = string.Empty;

    [Parameter]
    public int Count { get; set; } = 1;

    private bool _asked;
    private bool _verbose;

    /// <summary>
    /// Whether a verbose record would be kept, asked once. Both sides
    /// of the comparison ask, so neither is charged for a format the
    /// other skips.
    /// </summary>
    private bool VerboseOn
    {
        get
        {
            if (!_asked)
            {
                _verbose = MyInvocation.BoundParameters.TryGetValue("Verbose", out object? bound)
                    ? ((SwitchParameter)bound).IsPresent
                    : !"SilentlyContinue".Equals(
                        Convert.ToString(GetVariableValue("VerbosePreference")), StringComparison.OrdinalIgnoreCase);
                _asked = true;
            }
            return _verbose;
        }
    }

    protected override void ProcessRecord()
    {
        if (VerboseOn) WriteVerbose($"greeting {Name}");
        for (int i = 0; i < Count; i++) WriteObject($"Hello, {Name}!");
    }
}

[MemoryDiagnoser]
public class GreetingBenchmarks
{
    private Runspace _runspace = null!;

    [GlobalSetup]
    public void Setup()
    {
        var iss = InitialSessionState.CreateDefault2();
        iss.Commands.Add(new SessionStateCmdletEntry("Get-GreetingBaseline", typeof(GetGreetingBaselineCommand), null));
        string? module = System.Environment.GetEnvironmentVariable("PWRS_MODULE");
        if (module != null)
        {
            iss.ImportPSModule(new[] { System.IO.Path.Combine(module, "Hello.psd1") });
        }
        _runspace = RunspaceFactory.CreateRunspace(iss);
        _runspace.Open();
    }

    [GlobalCleanup]
    public void Cleanup() => _runspace.Dispose();

    private object Run(string script)
    {
        using var ps = PowerShell.Create();
        ps.Runspace = _runspace;
        ps.AddScript(script);
        return ps.Invoke();
    }

    [Benchmark(Baseline = true)]
    public object CSharpNoArg() => Run("1..1000 | ForEach-Object { Get-GreetingBaseline -Name x }");

    [Benchmark]
    public object PwrsNoArg() => Run("1..1000 | ForEach-Object { Get-Greeting -Name x }");

    [Benchmark]
    public object CSharpPipeline() => Run("1..1000 | Get-GreetingBaseline");

    [Benchmark]
    public object PwrsPipeline() => Run("1..1000 | Get-Greeting");
}
