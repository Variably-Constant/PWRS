using System.Collections.Generic;
using System.Management.Automation;

namespace Pwrs.Modules.Hello
{
    /// <summary>
    /// A hand-written cmdlet reading Pwrs.Cpu, the runtime's view of the CPU
    /// through the module's own library: the x86-64 level and the AArch64
    /// tier reached, the PWRS_CPU_MAX cap, and for each extension named
    /// whether a module may use it and whether the CPU offers it. Get-RustCpu
    /// returns the same values from Rust, and the two are compared.
    /// </summary>
    [Cmdlet(VerbsCommon.Get, "RustHybridCpu")]
    public sealed class GetRustHybridCpuCommand : PSCmdlet
    {
        [Parameter(Position = 0)]
        public string[] Extension { get; set; } = new string[0];

        protected override void EndProcessing()
        {
            var extensions = new List<PSObject>();
            foreach (string name in Extension)
            {
                var extension = new PSObject();
                extension.Properties.Add(new PSNoteProperty("Name", name));
                extension.Properties.Add(new PSNoteProperty("Has", global::Pwrs.Cpu.Has(name)));
                extension.Properties.Add(new PSNoteProperty("Detected", global::Pwrs.Cpu.Detected(name)));
                extensions.Add(extension);
            }
            var result = new PSObject();
            result.Properties.Add(new PSNoteProperty("Level", global::Pwrs.Cpu.Level));
            result.Properties.Add(new PSNoteProperty("ArmTier", global::Pwrs.Cpu.ArmTier));
            result.Properties.Add(new PSNoteProperty("Cap", global::Pwrs.Cpu.Cap));
            result.Properties.Add(new PSNoteProperty("Extensions", extensions.ToArray()));
            WriteObject(result);
        }
    }
}
