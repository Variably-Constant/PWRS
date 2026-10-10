using System.Management.Automation;

namespace Pwrs.Modules.Hello
{
    /// <summary>
    /// A second hand-written cmdlet, declaring its attributes in the
    /// forms the build's scanner accepts beside the plain one: the
    /// alias ahead of the cmdlet attribute, the cmdlet attribute
    /// fully qualified, and the alias names as an array.
    /// </summary>
    [Alias(new[] { "grhinfo" })]
    [System.Management.Automation.Cmdlet(VerbsCommon.Get, "RustHybridInfo")]
    public sealed class GetRustHybridInfoCommand : PSCmdlet
    {
        [Parameter(Position = 0)]
        public string Topic { get; set; } = "module";

        protected override void ProcessRecord() => WriteObject("hybrid info " + Topic);
    }
}
