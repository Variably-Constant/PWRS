using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace Pwrs
{
    /// <summary>
    /// Refuses a native library compiled for instruction-set extensions
    /// this machine does not offer, before any of its compiled code runs:
    /// the first such instruction to execute would end the process. The
    /// library lists what it was compiled for in the data export
    /// pwrs_cpu_requirements. On x86-64 the CPU is queried through
    /// pwrs_cpuid and pwrs_xgetbv, whose bodies are only those register
    /// instructions; each entry has its own CPUID leaf, subleaf, register
    /// and bit and the XCR0 state it needs. On AArch64 the features are
    /// read from the operating system, and each entry gives how that system
    /// reports the feature: AT_HWCAP bits on Linux, IsProcessorFeaturePresent
    /// constants on Windows, sysctl names on macOS. So this side has no
    /// table of its own. A feature the target compiles every crate with is
    /// listed at neon's tier and is not read, since every CPU that runs the
    /// target has it. A library built before the export existed is not
    /// checked.
    /// </summary>
    internal static unsafe class CpuCheck
    {
        private const string Marker = "PWRS-CPU/1";
        private const string ArmMarker = "PWRS-ARM/1";
        private const int NoCap = 5;
        private const int NoArmCap = 6;
        /// <summary>
        /// The tier an AArch64 list gives a feature the target compiles
        /// every crate with: neon's, the lowest, which no cap withholds.
        /// </summary>
        private const int ArmBaselineTier = 1;
        private const ulong AtHwcap = 16;
        private const ulong AtHwcap2 = 26;

#if !NET
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void CpuidFn(uint leaf, uint subleaf, uint* registers);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate ulong XgetbvFn(uint xcr);
#endif

        /// <summary>
        /// Throws when the library needs an extension this machine does
        /// not offer, or offers above the PWRS_CPU_MAX cap.
        /// </summary>
        internal static void Require(IntPtr lib, string path)
        {
            string? capName = Environment.GetEnvironmentVariable("PWRS_CPU_MAX");
            int cap = CapLevel(capName);
            IntPtr list = Loader.TryExportAddress(lib, "pwrs_cpu_requirements");
            if (list == IntPtr.Zero) return;
            string? text = Marshal.PtrToStringAnsi(list);
            if (text == null) return;
            string file = Path.GetFileName(path);
            if (text.StartsWith(ArmMarker, StringComparison.Ordinal))
            {
                string? refusal = ArmRefusal(text, file, cap, capName, RunningOs(), AuxVal, FeaturePresent, SysctlReadsNonzero);
                if (refusal != null) throw new PwrsException(refusal);
                return;
            }
            if (!text.StartsWith(Marker, StringComparison.Ordinal)) return;
            string[] entries = text.Substring(Marker.Length).Split(new[] { ' ' }, StringSplitOptions.RemoveEmptyEntries);
            if (entries.Length == 0) return;
            IntPtr cpuid = Loader.TryExportAddress(lib, "pwrs_cpuid");
            IntPtr xgetbv = Loader.TryExportAddress(lib, "pwrs_xgetbv");
            if (cpuid == IntPtr.Zero || xgetbv == IntPtr.Zero) return;

            var cpu = new Cpu(cpuid, xgetbv);
            var absent = new List<string>();
            var capped = new List<string>();
            foreach (string entry in entries)
            {
                string[] f = entry.Split(',');
                if (f.Length != 7) continue;
                int level = int.Parse(f[1], CultureInfo.InvariantCulture);
                uint leaf = uint.Parse(f[2], CultureInfo.InvariantCulture);
                uint subleaf = uint.Parse(f[3], CultureInfo.InvariantCulture);
                int register = int.Parse(f[4], CultureInfo.InvariantCulture);
                int bit = int.Parse(f[5], CultureInfo.InvariantCulture);
                ulong xcr0 = ulong.Parse(f[6], CultureInfo.InvariantCulture);
                if (!cpu.Has(leaf, subleaf, register, bit, xcr0))
                {
                    absent.Add(f[0]);
                }
                else if (level > cap)
                {
                    capped.Add(f[0]);
                }
            }
            if (absent.Count == 0 && capped.Count == 0) return;

            var parts = new List<string>();
            if (absent.Count > 0)
            {
                parts.Add("this CPU does not offer " + string.Join(", ", absent));
            }
            if (capped.Count > 0)
            {
                parts.Add("PWRS_CPU_MAX=" + capName!.Trim() + " withholds " + string.Join(", ", capped));
            }
            throw new PwrsException(
                file + " was compiled for instruction-set extensions it cannot use here: " + string.Join("; ", parts) + ". "
                + "A module compiled with -C target-cpu=native, or under a cargo config that sets it, runs only on CPUs like the one that built it. "
                + "Build it with RUSTFLAGS='-C target-cpu=x86-64' and choose wider kernels at run time with pwrs::cpu::has.");
        }

        /// <summary>
        /// The psABI level PWRS_CPU_MAX names, 1 for x86-64 to 4 for
        /// x86-64-v4, and 5 for no cap. On Arm64 it names a feature tier
        /// instead, 1 for neon to 5 for sve2, and 6 for no cap: the numbers
        /// pwrs::cpu gives the levels and tiers, which the requirement lists
        /// contain. A value naming nothing on this architecture is refused
        /// rather than read as no cap, so a mistyped cap cannot pass as a
        /// tested tier.
        /// </summary>
        internal static int CapLevel(string? value)
        {
            return CapLevel(value, RuntimeInformation.ProcessArchitecture == Architecture.Arm64);
        }

        /// <summary>CapLevel for an Arm64 process or any other.</summary>
        internal static int CapLevel(string? value, bool arm64)
        {
            int none = arm64 ? NoArmCap : NoCap;
            if (value == null) return none;
            string name = value.Trim();
            if (name.Length == 0 || name == "native") return none;
            if (arm64)
            {
                switch (name)
                {
                    case "neon":
                        return 1;
                    case "dotprod":
                        return 2;
                    case "i8mm":
                        return 3;
                    case "sve":
                        return 4;
                    case "sve2":
                        return 5;
                    default:
                        throw new PwrsException("PWRS_CPU_MAX is '" + value + "'; on Arm64 it takes neon, dotprod, i8mm, sve, sve2 or native.");
                }
            }
            switch (name)
            {
                case "x86-64":
                    return 1;
                case "x86-64-v2":
                    return 2;
                case "x86-64-v3":
                    return 3;
                case "x86-64-v4":
                    return 4;
                default:
                    throw new PwrsException("PWRS_CPU_MAX is '" + value + "'; it takes x86-64, x86-64-v2, x86-64-v3, x86-64-v4 or native.");
            }
        }

        /// <summary>
        /// The refusal an AArch64 requirement list earns here, or null when
        /// the operating system reports every feature it names beyond the
        /// target's baseline and the cap allows each. A list for another
        /// system than the one running is not checked. The three functions read the running system's
        /// reports: getauxval on Linux, IsProcessorFeaturePresent on Windows
        /// and whether a sysctl name reads nonzero on macOS, so a test can
        /// stand in for a system.
        /// </summary>
        internal static string? ArmRefusal(string text, string file, int cap, string? capName, string? os,
            Func<ulong, ulong> auxval, Func<uint, bool> present, Func<string, bool> sysctl)
        {
            string[] parts = text.Substring(ArmMarker.Length).Split(new[] { ' ' }, StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 2 || parts[0] != os || (os != "linux" && os != "windows" && os != "macos")) return null;
            bool read = false;
            ulong hwcap = 0;
            ulong hwcap2 = 0;
            var absent = new List<string>();
            var unreported = new List<string>();
            var unreadable = new List<string>();
            var capped = new List<string>();
            for (int i = 1; i < parts.Length; i++)
            {
                string[] f = parts[i].Split(',');
                if (f.Length != 3 || !int.TryParse(f[1], NumberStyles.None, CultureInfo.InvariantCulture, out int tier))
                {
                    unreadable.Add(parts[i]);
                    continue;
                }
                // Every CPU that runs the target has a baseline feature, and
                // a system's report of one can be missing, as macOS's names
                // for Advanced SIMD differ between versions, so it is not read.
                if (tier == ArmBaselineTier) continue;
                string name = f[0];
                string report = f[2];
                if (report.Length == 0)
                {
                    unreported.Add(name);
                    continue;
                }
                bool offered = true;
                bool understood = true;
                if (os == "linux")
                {
                    string[] l = report.Split(':');
                    if (l.Length != 3
                        || !ulong.TryParse(l[0], NumberStyles.None, CultureInfo.InvariantCulture, out ulong want)
                        || !ulong.TryParse(l[1], NumberStyles.None, CultureInfo.InvariantCulture, out ulong want2)
                        || (l[2] != "0" && l[2] != "1"))
                    {
                        understood = false;
                    }
                    else
                    {
                        if (!read)
                        {
                            hwcap = auxval(AtHwcap);
                            hwcap2 = auxval(AtHwcap2);
                            read = true;
                        }
                        // Half-precision floats (bit 9), where reported, must
                        // come with their Advanced SIMD form (bit 10).
                        bool halves = l[2] == "0" || (hwcap & (1UL << 9)) == 0 || (hwcap & (1UL << 10)) != 0;
                        offered = (hwcap & want) == want && (hwcap2 & want2) == want2 && halves;
                    }
                }
                else if (os == "windows")
                {
                    foreach (string constant in report.Split('+'))
                    {
                        if (!uint.TryParse(constant, NumberStyles.None, CultureInfo.InvariantCulture, out uint number)) understood = false;
                        else if (!present(number)) offered = false;
                    }
                }
                else
                {
                    foreach (string term in report.Split('+'))
                    {
                        bool met = false;
                        foreach (string ctl in term.Split('|'))
                        {
                            if (ctl.Length == 0) understood = false;
                            else if (!met && sysctl(ctl)) met = true;
                        }
                        if (!met) offered = false;
                    }
                }
                if (!understood) unreadable.Add(parts[i]);
                else if (!offered) absent.Add(name);
                else if (tier > cap) capped.Add(name);
            }
            if (absent.Count == 0 && unreported.Count == 0 && unreadable.Count == 0 && capped.Count == 0) return null;

            string system = os == "macos" ? "macOS" : os == "windows" ? "Windows" : "Linux";
            var reasons = new List<string>();
            if (absent.Count > 0)
            {
                reasons.Add("this CPU does not offer " + string.Join(", ", absent));
            }
            if (unreported.Count > 0)
            {
                reasons.Add(system + " has no way to report " + string.Join(", ", unreported) + ", so " + (unreported.Count == 1 ? "it" : "they") + " cannot be confirmed");
            }
            if (unreadable.Count > 0)
            {
                reasons.Add("its list has entries this runtime cannot read: " + string.Join(", ", unreadable));
            }
            if (capped.Count > 0)
            {
                reasons.Add("PWRS_CPU_MAX=" + capName!.Trim() + " withholds " + string.Join(", ", capped));
            }
            return file + " was compiled for AArch64 features it cannot use here: " + string.Join("; ", reasons) + ". "
                + "A module compiled with -C target-cpu=native, or with target features its target does not enable, runs only where each is confirmed. "
                + "Build it for its target's baseline and choose wider kernels at run time with pwrs::cpu::arm::has.";
        }

        /// <summary>The system an AArch64 list names for this process, or null for one no list names.</summary>
        private static string? RunningOs()
        {
            if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) return "windows";
            if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux)) return "linux";
            if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX)) return "macos";
            return null;
        }

        [DllImport("kernel32")]
        private static extern int IsProcessorFeaturePresent(uint feature);

        /// <summary>Whether Windows reports the PF_ constant.</summary>
        private static bool FeaturePresent(uint feature) => IsProcessorFeaturePresent(feature) != 0;

#if NET
        /// <summary>
        /// getauxval, from the libc the process already runs on, whatever
        /// its file is called.
        /// </summary>
        private static ulong AuxVal(ulong type)
        {
            var getauxval = (delegate* unmanaged[Cdecl]<nuint, nuint>)ProcessExport("getauxval");
            return getauxval((nuint)type);
        }

        /// <summary>
        /// Whether sysctlbyname reads the name as a nonzero integer, from
        /// the libSystem the process already runs on.
        /// </summary>
        private static bool SysctlReadsNonzero(string name)
        {
            var sysctlbyname = (delegate* unmanaged[Cdecl]<byte*, int*, nuint*, void*, nuint, int>)ProcessExport("sysctlbyname");
            byte[] utf8 = Encoding.UTF8.GetBytes(name + "\0");
            int value = 0;
            nuint length = sizeof(int);
            fixed (byte* p = utf8)
            {
                return sysctlbyname(p, &value, &length, null, 0) == 0 && value != 0;
            }
        }

        private static IntPtr ProcessExport(string name)
        {
            if (NativeLibrary.TryGetExport(NativeLibrary.GetMainProgramHandle(), name, out IntPtr export)) return export;
            throw new PwrsException("This process has no " + name + ", so the runtime cannot read which AArch64 features the CPU has.");
        }
#else
        // Windows PowerShell runs on Windows alone, where the check calls
        // IsProcessorFeaturePresent and neither of these is reached.
        private static ulong AuxVal(ulong type) => throw new PlatformNotSupportedException("getauxval is Linux's");

        private static bool SysctlReadsNonzero(string name) => throw new PlatformNotSupportedException("sysctlbyname is macOS's");
#endif

        private sealed class Cpu
        {
#if NET
            private readonly delegate* unmanaged[Cdecl]<uint, uint, uint*, void> _cpuid;
#else
            private readonly CpuidFn _cpuid;
#endif
            private readonly uint _maxBasic;
            private readonly uint _maxExtended;
            private readonly ulong _xcr0;

            internal Cpu(IntPtr cpuid, IntPtr xgetbv)
            {
#if NET
                _cpuid = (delegate* unmanaged[Cdecl]<uint, uint, uint*, void>)cpuid;
                var getbv = (delegate* unmanaged[Cdecl]<uint, ulong>)xgetbv;
#else
                _cpuid = Marshal.GetDelegateForFunctionPointer<CpuidFn>(cpuid);
                XgetbvFn getbv = Marshal.GetDelegateForFunctionPointer<XgetbvFn>(xgetbv);
#endif
                _maxBasic = Read(0, 0, 0);
                _maxExtended = Read(0x80000000, 0, 0);
                // XGETBV exists only when CPUID reports OSXSAVE.
                bool osxsave = _maxBasic >= 1 && ((Read(1, 0, 2) >> 27) & 1) == 1;
                _xcr0 = osxsave ? getbv(0) : 0;
            }

            private uint Read(uint leaf, uint subleaf, int register)
            {
                uint* r = stackalloc uint[4];
                _cpuid(leaf, subleaf, r);
                return r[register];
            }

            /// <summary>
            /// Whether CPUID reports the bit, for a leaf this CPU supports,
            /// with every XCR0 state bit the extension needs enabled.
            /// </summary>
            internal bool Has(uint leaf, uint subleaf, int register, int bit, ulong xcr0)
            {
                bool supported = leaf >= 0x80000000 ? leaf <= _maxExtended : leaf <= _maxBasic;
                if (!supported || register < 0 || register > 3 || bit < 0 || bit > 31) return false;
                if (((Read(leaf, subleaf, register) >> bit) & 1) == 0) return false;
                return (_xcr0 & xcr0) == xcr0;
            }
        }
    }
}
