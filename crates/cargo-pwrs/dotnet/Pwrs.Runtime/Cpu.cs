using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Pwrs
{
    /// <summary>
    /// The CPU as a PWRS module's native library sees it, so hand-written
    /// C# takes the same hardware path the Rust side takes: the x86-64 level
    /// or AArch64 feature tier reached, the cap PWRS_CPU_MAX sets, and
    /// whether an extension may be used. Each value comes from pwrs::cpu
    /// itself, through the library's pwrs_cpu_query export, so the two sides
    /// cannot disagree. The values come from the first library loaded in the
    /// process that has that export. A library reads the cap once, the first
    /// time it needs it, so libraries loaded under one cap return the same
    /// values; set it before the process imports a module.
    /// </summary>
    public static unsafe class Cpu
    {
        private const uint QueryLevel = 0;
        private const uint QueryArmTier = 1;
        private const uint QueryCap = 2;
        private const uint QueryHas = 3;
        private const uint QueryDetected = 4;
        private const int NoValue = -1;

#if NET
        private static delegate* unmanaged[Cdecl]<uint, byte*, nuint, byte*, nuint, int> s_query;
#else
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int QueryFn(uint query, byte* name, UIntPtr nameLength, byte* output, UIntPtr outputLength);
        private static QueryFn? s_query;
#endif
        private static readonly object s_gate = new object();

        /// <summary>
        /// Takes the library's export, unless an earlier library gave one.
        /// A library is never freed once loaded, so the export stays callable.
        /// </summary>
        internal static void Offer(IntPtr lib)
        {
            lock (s_gate)
            {
                if (s_query != null) return;
                IntPtr export = Loader.TryExportAddress(lib, "pwrs_cpu_query");
                if (export == IntPtr.Zero) return;
#if NET
                s_query = (delegate* unmanaged[Cdecl]<uint, byte*, nuint, byte*, nuint, int>)export;
#else
                s_query = Marshal.GetDelegateForFunctionPointer<QueryFn>(export);
#endif
            }
        }

        /// <summary>
        /// The x86-64 psABI level pwrs::cpu::level reaches under the cap, as
        /// PWRS_CPU_MAX names it, from "x86-64" to "x86-64-v4"; null on
        /// another architecture.
        /// </summary>
        public static string? Level => Name(QueryLevel);

        /// <summary>
        /// The AArch64 feature tier pwrs::cpu::arm::tier reaches under the
        /// cap, from "neon" to "sve2"; null on another architecture.
        /// </summary>
        public static string? ArmTier => Name(QueryArmTier);

        /// <summary>
        /// The cap PWRS_CPU_MAX sets on this architecture, as it names it: a
        /// level on x86-64, a tier on AArch64, and "native" for none.
        /// </summary>
        public static string? Cap => Name(QueryCap);

        /// <summary>
        /// Whether a module may use the extension, named as Rust's
        /// target_feature names it ("avx2", "sve"): the CPU and the operating
        /// system offer it, and it is at or below the cap. This is
        /// pwrs::cpu::has, or pwrs::cpu::arm::has on AArch64: the value to
        /// take a path by. The names are the ones pwrs::cpu tracks: the
        /// x86-64 extensions above the baseline, so not "sse2", and the
        /// AArch64 features, "neon" among them.
        /// </summary>
        /// <exception cref="ArgumentException">The name is not an extension of this architecture.</exception>
        public static bool Has(string extension) => Check(QueryHas, extension);

        /// <summary>
        /// Whether the CPU and the operating system offer the extension,
        /// whatever the cap.
        /// </summary>
        /// <exception cref="ArgumentException">The name is not an extension of this architecture.</exception>
        public static bool Detected(string extension) => Check(QueryDetected, extension);

        private static string? Name(uint query)
        {
            byte[] output = new byte[64];
            int length = Call(query, string.Empty, output);
            if (length == NoValue) return null;
            if (length < 0) throw Refused(query, length);
            return Encoding.UTF8.GetString(output, 0, length);
        }

        private static bool Check(uint query, string extension)
        {
            if (extension == null) throw new ArgumentNullException(nameof(extension));
            int result = Call(query, extension, new byte[0]);
            if (result == NoValue)
            {
                throw new ArgumentException("'" + extension + "' is not an extension pwrs::cpu knows on this architecture.", nameof(extension));
            }
            if (result < 0) throw Refused(query, result);
            return result == 1;
        }

        private static int Call(uint query, string name, byte[] output)
        {
            byte[] utf8 = Encoding.UTF8.GetBytes(name);
            fixed (byte* namePtr = utf8)
            fixed (byte* outputPtr = output)
            {
#if NET
                var export = s_query;
                if (export == null) throw NoLibrary();
                return export(query, namePtr, (nuint)utf8.Length, outputPtr, (nuint)output.Length);
#else
                QueryFn? export = s_query;
                if (export == null) throw NoLibrary();
                return export(query, namePtr, (UIntPtr)utf8.Length, outputPtr, (UIntPtr)output.Length);
#endif
            }
        }

        private static InvalidOperationException NoLibrary()
        {
            return new InvalidOperationException("Pwrs.Cpu reads the CPU through a PWRS module's library, and no library built with this version of PWRS is loaded in this process: import such a module first.");
        }

        private static InvalidOperationException Refused(uint query, int status)
        {
            return new InvalidOperationException("The module's library refused Pwrs.Cpu query " + query + " with status " + status + ".");
        }
    }
}
