using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;

namespace Pwrs
{
    /// <summary>
    /// Refuses a native library compiled for instruction-set extensions
    /// this machine does not offer, before any of its compiled code runs:
    /// the first such instruction to execute would end the process. The
    /// library lists what it was compiled for in the data export
    /// pwrs_cpu_requirements, and the CPU is asked through pwrs_cpuid and
    /// pwrs_xgetbv, whose bodies are only those register instructions.
    /// Each entry carries its own CPUID leaf, subleaf, register and bit
    /// and the XCR0 state it needs, so this side holds no table of its
    /// own. A library built before the exports existed is not checked.
    /// </summary>
    internal static unsafe class CpuCheck
    {
        private const string Marker = "PWRS-CPU/1";
        private const int NoCap = 5;

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
            if (text == null || !text.StartsWith(Marker, StringComparison.Ordinal)) return;
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

            string file = Path.GetFileName(path);
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
        /// x86-64-v4, and 5 for no cap. A value naming none of them is
        /// refused rather than read as no cap, so a mistyped cap cannot
        /// pass as a tested tier.
        /// </summary>
        internal static int CapLevel(string? value)
        {
            if (value == null) return NoCap;
            switch (value.Trim())
            {
                case "":
                case "native":
                    return NoCap;
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
            /// Whether CPUID reports the bit, for a leaf this CPU answers,
            /// with every XCR0 state bit the extension needs enabled.
            /// </summary>
            internal bool Has(uint leaf, uint subleaf, int register, int bit, ulong xcr0)
            {
                bool answered = leaf >= 0x80000000 ? leaf <= _maxExtended : leaf <= _maxBasic;
                if (!answered || register < 0 || register > 3 || bit < 0 || bit > 31) return false;
                if (((Read(leaf, subleaf, register) >> bit) & 1) == 0) return false;
                return (_xcr0 & xcr0) == xcr0;
            }
        }
    }
}
