using System.Management.Automation;

namespace Pwrs.Modules.Hello
{
    /// <summary>Hand-written C# beside the Rust cmdlets, exported with them.</summary>
    [Cmdlet(VerbsCommon.Get, "RustHybrid")]
    [Alias("grhyb")]
    public sealed class GetRustHybridCommand : PSCmdlet
    {
        [Parameter(Mandatory = true, Position = 0)]
        [Alias("n")]
        public string Name { get; set; } = string.Empty;

        protected override void ProcessRecord() => WriteObject("hybrid " + Name);
    }
}
