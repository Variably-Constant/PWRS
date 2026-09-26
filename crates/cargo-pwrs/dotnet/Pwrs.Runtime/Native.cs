using System;
using System.Diagnostics;
using System.Management.Automation;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading;

// Stack frames in this assembly are not zeroed on entry. No method
// here reads a local before assigning it and none uses stackalloc.
// The attribute is .NET only; netstandard2.0 has no such type.
#if NET
[module: SkipLocalsInit]
#endif

namespace Pwrs
{
    /// <summary>
    /// Timing counters for the managed half of a phase, printed to
    /// stderr every 10000 phases while PWRS_TRACE is set. Enabled is
    /// read once, so the untraced path is one static-readonly branch.
    /// </summary>
    public static class Trace
    {
        public static readonly bool Enabled = IsSet(Environment.GetEnvironmentVariable("PWRS_TRACE"));

        private const long SummaryEvery = 10000;
        private static long _phases;
        private static long _phaseTicks;
        private static long _nativeTicks;
        private static long _createTicks;
        private static long _creates;
        private static long _skipped;

        /// <summary>A Begin or End the learned mask let the shell skip.</summary>
        public static void Skipped() => Interlocked.Increment(ref _skipped);

        private static bool IsSet(string? v) => !string.IsNullOrEmpty(v) && v!.Trim() != "0";

        /// <summary>The clock the phase and native windows are timed against.</summary>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static long Now() => Stopwatch.GetTimestamp();

        /// <summary>One generated Run(phase) finished; started at t0.</summary>
        public static void Phase(long t0)
        {
            Interlocked.Add(ref _phaseTicks, Stopwatch.GetTimestamp() - t0);
            long n = Interlocked.Increment(ref _phases);
            if (n % SummaryEvery == 0) Report("phase");
        }

        /// <summary>The native invoke inside a phase finished; started at t0.</summary>
        public static void NativeCall(long t0) => Interlocked.Add(ref _nativeTicks, Stopwatch.GetTimestamp() - t0);

        /// <summary>One native instance creation finished; started at t0.</summary>
        public static void Create(long t0)
        {
            Interlocked.Add(ref _createTicks, Stopwatch.GetTimestamp() - t0);
            Interlocked.Increment(ref _creates);
        }

        private static long _lastPhases, _lastPhaseTicks, _lastNativeTicks, _lastCreates, _lastCreateTicks;

        private static long Window(ref long last, long now)
        {
            long delta = now - last;
            last = now;
            return delta;
        }

        /// <summary>
        /// Prints totals and the per-phase averages over the window
        /// since the previous report, so warm-up stays out of the
        /// steady-state figures.
        /// </summary>
        public static void Report(string what)
        {
            double nsPerTick = 1e9 / Stopwatch.Frequency;
            long phasesTotal = Interlocked.Read(ref _phases);
            long createsTotal = Interlocked.Read(ref _creates);
            long phases = Window(ref _lastPhases, phasesTotal);
            long creates = Window(ref _lastCreates, createsTotal);
            long phaseTicks = Window(ref _lastPhaseTicks, Interlocked.Read(ref _phaseTicks));
            long nativeTicks = Window(ref _lastNativeTicks, Interlocked.Read(ref _nativeTicks));
            long createTicks = Window(ref _lastCreateTicks, Interlocked.Read(ref _createTicks));
            long phaseNs = phases == 0 ? 0 : (long)(phaseTicks * nsPerTick / phases);
            long nativeNs = phases == 0 ? 0 : (long)(nativeTicks * nsPerTick / phases);
            long createNs = creates == 0 ? 0 : (long)(createTicks * nsPerTick / creates);
            long skipped = Interlocked.Read(ref _skipped);
            Console.Error.WriteLine($"pwrs trace managed {what}: phases={phasesTotal} skipped={skipped} creates={createsTotal} window: run_ns_avg={phaseNs} native_ns_avg={nativeNs} create_ns_avg={createNs}");
        }
    }

    /// <summary>Status codes and constants mirrored from pwrs-sys.</summary>
    public static class Native
    {
        public const uint AbiVersion = 1;

        public const int Ok = 0;
        public const int ErrManagedException = 1;
        public const int ErrNativePanic = 2;
        public const int ErrWrongThread = 3;
        public const int ErrAbiMismatch = 4;
        public const int ErrPipelineStopped = 5;

        public const uint PhaseBegin = 0;
        public const uint PhaseProcess = 1;
        public const uint PhaseEnd = 2;

        /// <summary>
        /// Bits pwrs_cmdlet_create reports: the phases the cmdlet type
        /// needs a native call for. All set until the native side has
        /// seen a phase run the trait's default body.
        /// </summary>
        public const uint PhaseMaskBegin = 1;
        public const uint PhaseMaskProcess = 2;
        public const uint PhaseMaskEnd = 4;
        public const uint PhaseMaskAll = 7;

        public const uint StreamVerbose = 1;
        public const uint StreamDebug = 2;
        public const uint StreamWarning = 3;
        public const uint StreamInformation = 4;

        public const uint TypeObject = 0;
        public const uint TypeBool = 1;
        public const uint TypeI8 = 2;
        public const uint TypeI16 = 3;
        public const uint TypeI32 = 4;
        public const uint TypeI64 = 5;
        public const uint TypeU8 = 6;
        public const uint TypeU16 = 7;
        public const uint TypeU32 = 8;
        public const uint TypeU64 = 9;
        public const uint TypeF32 = 10;
        public const uint TypeF64 = 11;
        public const uint TypeString = 12;
        public const uint TypeChar = 13;
        public const uint TypeDateTime = 14;
        public const uint TypeTimeSpan = 15;
        public const uint TypeGuid = 16;
        public const uint TypeDecimal = 17;

        /// <summary>Wraps an exception in a GCHandle for the err out-parameter.</summary>
        public static IntPtr Capture(Exception e) => GCHandle.ToIntPtr(GCHandle.Alloc(e));

        /// <summary>Takes the object behind an err handle and frees the handle.</summary>
        internal static object? TakeErr(IntPtr err) => TakeTarget(err);

        /// <summary>The object behind a handle; the handle stays allocated.</summary>
        public static object? TargetOf(IntPtr h) => h == IntPtr.Zero ? null : GCHandle.FromIntPtr(h).Target;

        /// <summary>The object behind a handle; the handle is freed.</summary>
        public static object? TakeTarget(IntPtr h)
        {
            if (h == IntPtr.Zero) return null;
            var handle = GCHandle.FromIntPtr(h);
            object? o = handle.Target;
            handle.Free();
            return o;
        }

        /// <summary>The tag of an element type, TypeObject when it has none.</summary>
        internal static uint ElementTag(Type elem)
        {
            if (elem == typeof(bool)) return TypeBool;
            if (elem == typeof(sbyte)) return TypeI8;
            if (elem == typeof(short)) return TypeI16;
            if (elem == typeof(int)) return TypeI32;
            if (elem == typeof(long)) return TypeI64;
            if (elem == typeof(byte)) return TypeU8;
            if (elem == typeof(ushort)) return TypeU16;
            if (elem == typeof(uint)) return TypeU32;
            if (elem == typeof(ulong)) return TypeU64;
            if (elem == typeof(float)) return TypeF32;
            if (elem == typeof(double)) return TypeF64;
            if (elem == typeof(string)) return TypeString;
            if (elem == typeof(char)) return TypeChar;
            if (elem == typeof(DateTime)) return TypeDateTime;
            if (elem == typeof(TimeSpan)) return TypeTimeSpan;
            if (elem == typeof(Guid)) return TypeGuid;
            if (elem == typeof(decimal)) return TypeDecimal;
            return TypeObject;
        }

        internal static Type ElementType(uint tag) => tag switch
        {
            TypeBool => typeof(bool),
            TypeI8 => typeof(sbyte),
            TypeI16 => typeof(short),
            TypeI32 => typeof(int),
            TypeI64 => typeof(long),
            TypeU8 => typeof(byte),
            TypeU16 => typeof(ushort),
            TypeU32 => typeof(uint),
            TypeU64 => typeof(ulong),
            TypeF32 => typeof(float),
            TypeF64 => typeof(double),
            TypeString => typeof(string),
            TypeChar => typeof(char),
            TypeDateTime => typeof(DateTime),
            TypeTimeSpan => typeof(TimeSpan),
            TypeGuid => typeof(Guid),
            TypeDecimal => typeof(decimal),
            _ => typeof(object),
        };
    }

    /// <summary>Borrowed UTF-16 string; mirrors pwrs_sys::PsStr16.</summary>
    [StructLayout(LayoutKind.Sequential)]
    public unsafe struct PsStr16
    {
        public ushort* Ptr;
        public nuint Len;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public override string ToString() => Ptr == null || Len == 0 ? string.Empty : new string((char*)Ptr, 0, checked((int)Len));
    }

    /// <summary>Pinned primitive array view; mirrors pwrs_sys::PsPinned.</summary>
    [StructLayout(LayoutKind.Sequential)]
    public unsafe struct PsPinned
    {
        public void* Data;
        public nuint Len;
        public uint ElemSize;
        public IntPtr Pin;
    }

    /// <summary>Exception carrying an error record raised by Rust.</summary>
    public sealed class PwrsException : Exception
    {
        public PwrsException(string message) : base(message) { }
    }
}
