using System;
using System.Diagnostics;
using System.Management.Automation;
using System.Reflection;
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

        /// <summary>A Begin or End the phase mask let the shell skip.</summary>
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

        /// <summary>How task_settle finishes a task; mirrors pwrs_sys::PS_TASK_*.</summary>
        public const uint TaskValue = 0;
        public const uint TaskFault = 1;
        public const uint TaskCancel = 2;

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

    /// <summary>
    /// The task a method passes to its Rust half; mirrors pwrs_sys::PsTaskSlot:
    /// a handle to the <see cref="TaskSource"/> and the byte it sets once
    /// the token is canceled.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public unsafe struct PsTaskSlot
    {
        public IntPtr Source;
        public byte* Cancelled;
    }

    /// <summary>
    /// Exception with an error record raised by Rust; its
    /// InnerException has the error's details when it has any. Its
    /// ErrorRecord is the record PowerShell reports for it. One made from
    /// the error of a provider method or a lifecycle hook has the error's
    /// id, category and target, and, when the error names an exception
    /// type, that exception as the record's exception, which a script's
    /// catch of that type catches. Any other has the exception's type name
    /// as its id, no category and no target, the record PowerShell makes
    /// for an exception that has none.
    /// </summary>
    public sealed class PwrsException : Exception, IContainsErrorRecord
    {
        private readonly string _id = typeof(PwrsException).FullName!;
        private readonly ErrorCategory _category = ErrorCategory.NotSpecified;
        private readonly object? _target;
        private readonly Exception? _shown;
        private ErrorRecord? _record;

        public PwrsException(string message) : base(message) { }

        public PwrsException(string message, Exception inner) : base(message, inner) { }

        internal PwrsException(string message, Exception? inner, string id, ErrorCategory category, object? target, Exception? shown) : base(message, inner)
        {
            _id = id;
            _category = category;
            _target = target;
            _shown = shown;
        }

        public ErrorRecord ErrorRecord => _record ??= new ErrorRecord(_shown ?? this, _id, _category, _target);

        /// <summary>
        /// The exception for an error a provider method or a lifecycle hook
        /// returned, from its cells [message, id, category, details, target]
        /// and, for an error that names its exception type, a sixth cell with
        /// the exception <see cref="NamedException.Make"/> made. It has the
        /// message, and a record with the error's id and category and its
        /// target, or <paramref name="target"/> for an error that has none.
        /// Its InnerException is the named exception, which has the details
        /// inside it and is the record's exception, so a catch of the named
        /// type finds it; without one, the InnerException has the details.
        /// </summary>
        internal static PwrsException FromCells(object?[] cells, object? target)
        {
            if (cells.Length < 5) return new PwrsException($"an error from Rust needs [message, id, category, details, target]; got {cells.Length} cells");
            Exception? details = cells[3] == null ? null : new PwrsException(Text(cells[3]));
            var category = (ErrorCategory)LanguagePrimitives.ConvertTo<int>(cells[2] is PSObject ps ? ps.BaseObject : cells[2]);
            Exception? shown = cells.Length > 5 ? (cells[5] is PSObject named ? named.BaseObject : cells[5]) as Exception : null;
            object? recordTarget = cells[4] ?? target;
            if (shown != null) NamedException.SetOwnRecord(shown, Text(cells[1]), category, recordTarget);
            return new PwrsException(Text(cells[0]), shown ?? details, Text(cells[1]), category, recordTarget, shown);
        }

        private static string Text(object? o) => (o is PSObject ps ? ps.BaseObject : o)?.ToString() ?? string.Empty;
    }

    /// <summary>
    /// The exception of a type a Rust error names. A call throws it as it
    /// is, and a record carries it as the record's exception.
    /// </summary>
    internal static class NamedException
    {
        /// <summary>The Data key holding the error's id.</summary>
        internal const string IdKey = "PwrsErrorId";

        /// <summary>The Data key holding the name of the error's category.</summary>
        internal const string CategoryKey = "PwrsErrorCategory";

        /// <summary>
        /// An exception of the type <paramref name="typeName"/> names,
        /// resolved as PowerShell resolves a type literal, made with its
        /// (string, Exception) constructor: <paramref name="message"/>, and
        /// an inner exception with <paramref name="details"/> when they are
        /// not empty. A type with no such constructor is made with its
        /// (string) one when there are no details. Its Data holds the id and
        /// the category's name. A name that gives no exception type, or one
        /// these constructors cannot make, gives a PwrsException with the
        /// message and why. PwrsException's own name gives this runtime's
        /// PwrsException, whose record has the id and the category, without
        /// resolving the name: two modules loaded in one process each have a
        /// type of that name. Never throws.
        /// </summary>
        internal static Exception Make(string typeName, string message, string id, ErrorCategory category, string details)
        {
            Exception? inner = details.Length == 0 ? null : new PwrsException(details);
            Exception made;
            if (typeName == typeof(PwrsException).FullName)
            {
                made = new PwrsException(message, inner, id, category, null, null);
            }
            else
            {
                try
                {
                    made = Construct(typeName, message, inner);
                }
                catch (Exception e)
                {
                    string why = e is TargetInvocationException { InnerException: { } thrown } ? thrown.Message : e.Message;
                    made = inner == null
                        ? new PwrsException($"{message} (the error names the exception type {typeName}, which could not be made: {why})")
                        : new PwrsException($"{message} (the error names the exception type {typeName}, which could not be made: {why})", inner);
                }
            }
            try
            {
                made.Data[IdKey] = id;
                made.Data[CategoryKey] = category.ToString();
            }
            catch (Exception)
            {
                // A type whose Data refuses entries keeps its message, which
                // starts with the id for an error a call returned.
            }
            SetOwnRecord(made, id, category, null);
            return made;
        }

        /// <summary>
        /// Gives an exception that carries its own record, as every
        /// RuntimeException does, a record with the error's id, category
        /// and target, which is what PowerShell reports for a catch of its
        /// type. RuntimeException and SessionStateException each store the
        /// record they return in an _errorRecord field and make it only
        /// while that is null, in pwsh 7 and Windows PowerShell 5.1, so
        /// the most derived such field is the one set. An exception without
        /// one keeps its own record.
        /// </summary>
        internal static void SetOwnRecord(Exception exception, string id, ErrorCategory category, object? target)
        {
            if (exception is not IContainsErrorRecord) return;
            for (Type? type = exception.GetType(); type != null && type != typeof(Exception); type = type.BaseType)
            {
                FieldInfo? stored = type.GetField("_errorRecord", BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.DeclaredOnly);
                if (stored == null || stored.FieldType != typeof(ErrorRecord)) continue;
                try
                {
                    stored.SetValue(exception, new ErrorRecord(new ParentContainsErrorRecordException(exception), id, category, target));
                }
                catch (Exception)
                {
                    // The exception keeps the record it makes for itself.
                }
                return;
            }
        }

        private static Exception Construct(string typeName, string message, Exception? inner)
        {
            Type type;
            try
            {
                type = LanguagePrimitives.ConvertTo<Type>(typeName);
            }
            catch (Exception)
            {
                throw new ArgumentException($"no type is named {typeName}");
            }
            if (!typeof(Exception).IsAssignableFrom(type)) throw new ArgumentException($"{type.FullName} is not an exception type");
            ConstructorInfo? withInner = type.GetConstructor(new[] { typeof(string), typeof(Exception) });
            if (withInner != null) return (Exception)withInner.Invoke(new object?[] { message, inner });
            ConstructorInfo? plain = inner == null ? type.GetConstructor(new[] { typeof(string) }) : null;
            if (plain != null) return (Exception)plain.Invoke(new object[] { message });
            throw new ArgumentException(inner == null
                ? $"{type.FullName} has no public constructor taking a message"
                : $"{type.FullName} has no public constructor taking a message and an inner exception");
        }

        /// <summary>
        /// The id and category a named exception's Data holds, or false for
        /// an exception no Rust error named.
        /// </summary>
        internal static bool TryRecordOf(Exception exception, out string id, out ErrorCategory category)
        {
            id = string.Empty;
            category = ErrorCategory.NotSpecified;
            if (exception.Data[IdKey] is not string named) return false;
            id = named;
            if (exception.Data[CategoryKey] is string name && Enum.TryParse(name, out ErrorCategory parsed)) category = parsed;
            return true;
        }
    }
}
