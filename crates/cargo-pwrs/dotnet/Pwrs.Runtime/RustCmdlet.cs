using System;
using System.Runtime.InteropServices;
using System.Management.Automation;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// Base class of every generated cmdlet. The generated subclass
    /// carries the [Cmdlet] attribute, the [Parameter] properties, the
    /// cmdlet id, and a PackParameters method that writes the
    /// parameter block; this class owns the handle to itself, the
    /// pipeline-thread identity, the pending terminating error, and
    /// the single native call per phase.
    /// </summary>
    public abstract class RustCmdlet : PSCmdlet, IDisposable
    {
        protected abstract uint CmdletId { get; }
        protected abstract NativeModule Module { get; }

        private GCHandle _self;
        private IntPtr _selfHandle;
        private int _pipelineThreadId;
        private ErrorRecord? _pendingTerminating;
        private bool _stopped;

        private IntPtr _instance;
        private uint _phases = Native.PhaseMaskAll;

        /// <summary>
        /// True when the native side still needs a call for the phase
        /// in <paramref name="mask"/>. Creates the instance if it does
        /// not exist yet, since the mask arrives with it.
        /// </summary>
        protected bool NeedsPhase(uint mask)
        {
            IntPtr instance = Instance;
            if ((_phases & mask) != 0) return true;
            if (Trace.Enabled) Trace.Skipped();
            return false;
        }

        /// <summary>
        /// Handle to this cmdlet, allocated on first use and freed in
        /// Dispose. The pointer is kept beside the handle, since it
        /// cannot change once allocated.
        /// </summary>
        protected internal IntPtr SelfHandle
        {
            get
            {
                if (_selfHandle == IntPtr.Zero)
                {
                    _self = GCHandle.Alloc(this);
                    _selfHandle = GCHandle.ToIntPtr(_self);
                }
                return _selfHandle;
            }
        }

        /// <summary>
        /// The Rust instance this cmdlet owns for its lifetime. Holding
        /// it here is what keeps the per-invocation path free of any
        /// shared table: the native side is handed the pointer and does
        /// no lookup.
        /// </summary>
        private IntPtr Instance
        {
            get
            {
                if (_instance == IntPtr.Zero)
                {
                    long t0 = Trace.Enabled ? Trace.Now() : 0;
                    _instance = Module.CmdletCreate(CmdletId, out _phases);
                    if (Trace.Enabled) Trace.Create(t0);
                }
                return _instance;
            }
        }

        /// <summary>
        /// Every vtable entry that touches a stream or session state asks
        /// this first, so it runs per output write rather than per phase.
        /// </summary>
        internal bool OnPipelineThread => Environment.CurrentManagedThreadId == _pipelineThreadId;

        private uint _streamsAsked;
        private uint _streamsOn;

        /// <summary>
        /// Whether the engine would keep a record written to this stream.
        /// A verbose or debug record is kept only where it is shown. A
        /// warning or information record is also kept under
        /// SilentlyContinue, where -WarningVariable, -InformationVariable
        /// and an information redirection receive it, and under Ignore
        /// when its -*Variable parameter is bound. Answered once per
        /// stream and remembered: the common parameters are bound when
        /// the command is invoked, so the answer cannot change while this
        /// instance runs.
        /// </summary>
        internal bool StreamEnabled(uint kind)
        {
            uint bit = 1u << (int)kind;
            if ((_streamsAsked & bit) == 0)
            {
                if (ComputeStreamEnabled(kind)) _streamsOn |= bit;
                _streamsAsked |= bit;
            }
            return (_streamsOn & bit) != 0;
        }

        private bool ComputeStreamEnabled(uint kind)
        {
            string parameter, preference;
            string? variable = null;
            switch (kind)
            {
                case Native.StreamVerbose: parameter = "Verbose"; preference = "VerbosePreference"; break;
                case Native.StreamDebug: parameter = "Debug"; preference = "DebugPreference"; break;
                case Native.StreamWarning: parameter = "WarningAction"; preference = "WarningPreference"; variable = "WarningVariable"; break;
                case Native.StreamInformation: parameter = "InformationAction"; preference = "InformationPreference"; variable = "InformationVariable"; break;
                default: throw new ArgumentOutOfRangeException(nameof(kind), $"unknown stream kind {kind}");
            }
            bool keptWhenSilent = variable != null;
            if (variable != null && MyInvocation.BoundParameters.ContainsKey(variable))
            {
                return true;
            }
            // The common parameter wins wherever it is bound; -Verbose and
            // -Debug arrive as switches, the other two as an ActionPreference.
            if (MyInvocation.BoundParameters.TryGetValue(parameter, out object? bound) && Keeps(bound, keptWhenSilent, out bool byParameter))
            {
                return byParameter;
            }
            return Keeps(GetVariableValue(preference), keptWhenSilent, out bool byPreference) && byPreference;
        }

        /// <summary>
        /// Reads one preference value; false when it says nothing.
        /// <paramref name="keptWhenSilent"/> is whether SilentlyContinue
        /// still keeps a record of this stream.
        /// </summary>
        private static bool Keeps(object? value, bool keptWhenSilent, out bool kept)
        {
            kept = false;
            switch (value)
            {
                case null: return false;
                case SwitchParameter sw: kept = sw.IsPresent; return true;
                case bool b: kept = b; return true;
                case ActionPreference preference: kept = KeptUnder(preference, keptWhenSilent); return true;
                default:
                    // A preference variable can hold the name or the number.
                    if (LanguagePrimitives.TryConvertTo(value, out ActionPreference parsed))
                    {
                        kept = KeptUnder(parsed, keptWhenSilent);
                        return true;
                    }
                    return false;
            }
        }

        private static bool KeptUnder(ActionPreference preference, bool keptWhenSilent) =>
            preference != ActionPreference.Ignore
            && (keptWhenSilent || preference != ActionPreference.SilentlyContinue);

        internal void SetPendingTerminating(ErrorRecord record) => _pendingTerminating = record;

        internal void MarkStopped() => _stopped = true;

        /// <summary>Runs one phase: one native call with the packed parameter block.</summary>
        protected unsafe void Invoke(uint phase, void* parameters)
        {
            _pipelineThreadId = Environment.CurrentManagedThreadId;
            _pendingTerminating = null;
            _stopped = false;
            IntPtr err = IntPtr.Zero;
            IntPtr instance = Instance;
            long t0 = Trace.Enabled ? Trace.Now() : 0;
            int status = Module.CmdletInvoke(instance, phase, SelfHandle, (IntPtr)parameters, &err);
            if (Trace.Enabled) Trace.NativeCall(t0);
            object? errObj = Native.TakeErr(err);

            if (_stopped || status == Native.ErrPipelineStopped) throw new PipelineStoppedException();
            if (_pendingTerminating != null)
            {
                var rec = _pendingTerminating;
                _pendingTerminating = null;
                ThrowTerminatingError(rec);
            }
            switch (status)
            {
                case Native.Ok:
                    return;
                case Native.ErrNativePanic:
                    ThrowTerminatingError(new ErrorRecord(
                        new PwrsException("native module panicked: " + (errObj as string ?? "no message")),
                        "PwrsNativePanic", ErrorCategory.InvalidOperation, null));
                    return;
                default:
                    ThrowTerminatingError(new ErrorRecord(
                        errObj as Exception ?? new PwrsException($"pwrs runtime call failed with status {status}"),
                        "PwrsRuntimeError", ErrorCategory.InvalidOperation, null));
                    return;
            }
        }

        /// <summary>
        /// Called on the engine's thread while the pipeline thread runs.
        /// Reads the instance pointer without clearing it; Dispose is
        /// what clears it, and the engine does not dispose a cmdlet it
        /// is still stopping.
        /// </summary>
        protected override void StopProcessing()
        {
            IntPtr instance = _instance;
            if (instance != IntPtr.Zero) Module.CmdletStop(instance);
        }

        public void Dispose()
        {
            IntPtr instance = Interlocked.Exchange(ref _instance, IntPtr.Zero);
            if (instance != IntPtr.Zero) Module.CmdletRelease(instance);
            if (_self.IsAllocated) _self.Free();
            _selfHandle = IntPtr.Zero;
            GC.SuppressFinalize(this);
        }
    }
}
