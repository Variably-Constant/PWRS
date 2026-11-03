using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Reflection;
using System.Runtime.InteropServices;

namespace Pwrs
{
    /// <summary>
    /// Base class of a generated cmdlet that wraps another command, as a
    /// proxy command does. The generated subclass declares the command's
    /// parameters: as properties where every PowerShell the module loads in
    /// declares them alike, and through <see cref="Declare"/> at bind time
    /// where the running PowerShell declares them its own way. The native
    /// side drives the command through the host table's wrapped entries,
    /// which this class serves: the arguments the caller bound to the
    /// copied parameters, the record as it was piped, and the command as a
    /// steppable pipeline whose output goes through the native side's hook.
    /// </summary>
    public abstract unsafe class WrappedCmdlet : RustCmdlet, IDynamicParameters
    {
        /// <summary>The name of the command this cmdlet wraps.</summary>
        protected abstract string WrappedName { get; }

        /// <summary>
        /// Whether the wrapped command adds parameters of its own at bind
        /// time, as a provider cmdlet does for the path given, which this
        /// cmdlet then takes from it and declares too.
        /// </summary>
        protected abstract bool ForwardsDynamic { get; }

        /// <summary>
        /// Whether <paramref name="name"/>, lowercased, is a parameter the
        /// subclass copies from the wrapped command as a property.
        /// </summary>
        protected abstract bool IsCopiedProperty(string name);

        /// <summary>
        /// Declares, through <see cref="Add"/>, the parameters the wrapped
        /// command's reading for the running engine declares its own way:
        /// <paramref name="major"/> and <paramref name="minor"/> are the
        /// engine's version, and <paramref name="windows"/> whether it runs
        /// on Windows.
        /// </summary>
        protected abstract void Declare(int major, int minor, bool windows);

        /// <summary>
        /// Why this cmdlet does not run the wrapped command in this
        /// PowerShell, or null when it does.
        /// </summary>
        protected virtual string? Refusal => null;

        internal string Name => WrappedName;

        private RuntimeDefinedParameterDictionary? _declaring;
        private readonly HashSet<string> _declared = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        private HashSet<string>? _running;
        private SteppablePipeline? _pipeline;
        private IntPtr _hook;
        private IntPtr _hookContext;

        /// <summary>
        /// The parameters the copies declare for the running engine, those
        /// of the reading for its version and platform, then the wrapped
        /// command's own dynamic parameters for the arguments bound so far.
        /// Null when there are none.
        /// </summary>
        public object? GetDynamicParameters()
        {
            var parameters = new RuntimeDefinedParameterDictionary();
            _declaring = parameters;
            _declared.Clear();
            try
            {
                if (Refusal == null)
                {
                    Running();
                    Version engine = typeof(PSObject).Assembly.GetName().Version ?? new Version(0, 0);
                    Declare(engine.Major, engine.Minor, RuntimeInformation.IsOSPlatform(OSPlatform.Windows));
                    if (ForwardsDynamic) DeclareForwarded(parameters);
                }
            }
            finally
            {
                _declaring = null;
            }
            return parameters.Count == 0 ? null : parameters;
        }

        /// <summary>Whether the engine's version is at least <paramref name="major"/>.<paramref name="minor"/>.</summary>
        protected static bool AtLeast(int engineMajor, int engineMinor, int major, int minor) =>
            engineMajor > major || (engineMajor == major && engineMinor >= minor);

        /// <summary>The type a runtime name names, or null where the engine has none.</summary>
        protected static Type? TypeNamed(string name) => Type.GetType(name, throwOnError: false);

        /// <summary>
        /// The type a runtime name names, for an attribute argument; a
        /// missing one leaves the attribute off, through <see cref="Make"/>.
        /// </summary>
        protected static Type Need(string name) => TypeNamed(name) ?? throw new MissingTypeException();

        /// <summary>An attribute <paramref name="make"/> builds, or null when it names a type the engine lacks.</summary>
        protected static Attribute? Make(Func<Attribute> make)
        {
            try
            {
                return make();
            }
            catch (MissingTypeException)
            {
                return null;
            }
        }

        /// <summary>
        /// Declares the parameter <paramref name="name"/> of type
        /// <paramref name="type"/> with the attributes given, when the
        /// running command has it and the engine has its type.
        /// </summary>
        protected void Add(string name, Type? type, params Attribute?[] attributes)
        {
            if (_declaring == null || type == null || !RunningHas(name)) return;
            var kept = new Collection<Attribute>();
            foreach (Attribute? a in attributes)
            {
                if (a != null) kept.Add(a);
            }
            _declaring[name] = new RuntimeDefinedParameter(name, type, kept);
            _declared.Add(name);
        }

        /// <summary>Whether the running engine's wrapped command declares <paramref name="name"/> itself.</summary>
        private bool RunningHas(string name) => Running().Contains(name);

        /// <summary>
        /// The names of the parameters the running engine's wrapped command
        /// declares itself. Resolving the command loads the module it comes
        /// from, so a type that module's assembly defines resolves by name.
        /// </summary>
        private HashSet<string> Running()
        {
            if (_running == null)
            {
                _running = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
                CommandInfo? command = Resolved();
                if (command != null)
                {
                    foreach (string key in new CommandMetadata(command).Parameters.Keys) _running.Add(key);
                }
            }
            return _running;
        }

        /// <summary>The wrapped command, an alias followed to what it names, or null where there is none.</summary>
        private CommandInfo? Resolved()
        {
            CommandInfo? info = InvokeCommand.GetCommand(WrappedName, CommandTypes.All);
            while (info is AliasInfo alias && alias.ResolvedCommand != null) info = alias.ResolvedCommand;
            return info;
        }

        /// <summary>
        /// Declares each dynamic parameter the wrapped command adds for the
        /// copied arguments bound so far that this cmdlet does not declare
        /// already, with the command's own type and attributes. Its own
        /// parameters are not the command's to resolve.
        /// </summary>
        private void DeclareForwarded(RuntimeDefinedParameterDictionary parameters)
        {
            CommandInfo? command = InvokeCommand.GetCommand(WrappedName, CommandTypes.All, new object[] { Arguments() });
            if (command == null) return;
            foreach (ParameterMetadata p in command.Parameters.Values)
            {
                if (!p.IsDynamic || parameters.ContainsKey(p.Name) || IsCopiedProperty(p.Name.ToLowerInvariant())) continue;
                parameters.Add(p.Name, new RuntimeDefinedParameter(p.Name, p.ParameterType, p.Attributes));
                _declared.Add(p.Name);
            }
        }

        /// <summary>The arguments the caller bound to the parameters copied from the wrapped command, by name.</summary>
        internal Hashtable Arguments()
        {
            var arguments = new Hashtable(StringComparer.OrdinalIgnoreCase);
            foreach (KeyValuePair<string, object> pair in MyInvocation.BoundParameters)
            {
                if (IsCopiedProperty(pair.Key.ToLowerInvariant()) || _declared.Contains(pair.Key)) arguments[pair.Key] = pair.Value;
            }
            return arguments;
        }

        /// <summary>The engine's own record of the object it passes to the command for this record, before binding.</summary>
        private static readonly PropertyInfo? CurrentPipelineObject =
            typeof(Cmdlet).BaseType?.GetProperty("CurrentPipelineObject", BindingFlags.NonPublic | BindingFlags.Instance);

        /// <summary>
        /// The record the pipeline passes to this cmdlet now, as it was piped,
        /// or false when the cmdlet takes no pipeline input.
        /// </summary>
        internal bool TryInput(out object? record)
        {
            record = null;
            if (!MyInvocation.ExpectingInput) return false;
            if (CurrentPipelineObject == null)
            {
                throw new PwrsException("pwrs: this PowerShell has no InternalCommand.CurrentPipelineObject, from which a wrapping cmdlet reads the record piped to it");
            }
            record = CurrentPipelineObject.GetValue(this, null);
            return true;
        }

        /// <summary>Throws the reason this cmdlet does not run the wrapped command, when it has one.</summary>
        internal void CheckRefusal()
        {
            string? refusal = Refusal;
            if (refusal != null) throw new PwrsException(refusal);
        }

        /// <summary>The last stage of the wrapped command's pipeline, which passes its output to <see cref="Deliver"/>.</summary>
        private static readonly CmdletInfo OutputStage = new CmdletInfo("Out-PwrsWrapped", typeof(WrappedOutputCommand));

        /// <summary>
        /// Begins the wrapped command with <paramref name="arguments"/>
        /// splatted, as a steppable pipeline whose output and streams are
        /// this cmdlet's, its output passing through the hook.
        /// </summary>
        internal void Begin(Hashtable arguments, IntPtr hook, IntPtr context)
        {
            ScriptBlock script = ScriptBlock.Create("param($c, $a, $h, $o) & $c @a | & $h -Owner $o");
            SteppablePipeline pipeline = script.GetSteppablePipeline(MyInvocation.CommandOrigin, new object[] { WrappedName, arguments, OutputStage, this });
            _pipeline = pipeline;
            Step(hook, context, () => pipeline.Begin(this));
        }

        /// <summary>Process(record), or Process() without one, its output passing through the hook.</summary>
        internal void Process(object? record, bool hasRecord, IntPtr hook, IntPtr context)
        {
            SteppablePipeline pipeline = _pipeline ?? throw new InvalidOperationException("pwrs: the wrapped command was not begun");
            if (hasRecord) Step(hook, context, () => pipeline.Process(record));
            else Step(hook, context, () => pipeline.Process());
        }

        /// <summary>End(), its output passing through the hook, then the pipeline's clean blocks and disposal.</summary>
        internal void End(IntPtr hook, IntPtr context)
        {
            SteppablePipeline pipeline = _pipeline ?? throw new InvalidOperationException("pwrs: the wrapped command was not begun");
            try
            {
                Step(hook, context, () => pipeline.End());
            }
            finally
            {
                Close();
            }
        }

        private void Step(IntPtr hook, IntPtr context, Action step)
        {
            _hook = hook;
            _hookContext = context;
            try
            {
                step();
            }
            finally
            {
                _hook = IntPtr.Zero;
                _hookContext = IntPtr.Zero;
            }
        }

        /// <summary>Runs the pipeline's clean blocks where the engine has them, then disposes it.</summary>
        private void Close()
        {
            SteppablePipeline? pipeline = _pipeline;
            _pipeline = null;
            if (pipeline == null) return;
            try
            {
#if NET
                pipeline.Clean();
#endif
            }
            finally
            {
                pipeline.Dispose();
            }
        }

        /// <summary>A wrapped command that has not ended is closed with the cmdlet, as the engine closes a stopped pipeline.</summary>
        internal override void OnDispose() => Close();

#if NET
        private static int CallHook(IntPtr hook, IntPtr context, IntPtr record, IntPtr* err) =>
            ((delegate* unmanaged[Cdecl]<IntPtr, IntPtr, IntPtr*, int>)hook)(context, record, err);
#else
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int HookFn(IntPtr context, IntPtr record, IntPtr* err);
        private static IntPtr _lastHook;
        private static HookFn? _lastHookFn;

        private static int CallHook(IntPtr hook, IntPtr context, IntPtr record, IntPtr* err)
        {
            HookFn? fn = _lastHookFn;
            if (fn == null || _lastHook != hook)
            {
                fn = Marshal.GetDelegateForFunctionPointer<HookFn>(hook);
                _lastHookFn = fn;
                _lastHook = hook;
            }
            return fn(context, record, err);
        }
#endif

        /// <summary>
        /// Passes one record the wrapped command wrote to the native hook. A
        /// stop goes on as one; a panic as an exception with its message;
        /// any other failure aborts the command with a
        /// <see cref="WrappedHookFailed"/>, the native side storing its
        /// error to report itself.
        /// </summary>
        internal void Deliver(object? record)
        {
            if (_hook == IntPtr.Zero)
            {
                WriteObject(record);
                return;
            }
            GCHandle handle = GCHandle.Alloc(record);
            IntPtr err = IntPtr.Zero;
            int status;
            try
            {
                status = CallHook(_hook, _hookContext, GCHandle.ToIntPtr(handle), &err);
            }
            finally
            {
                handle.Free();
            }
            if (status == Native.Ok) return;
            object? error = Native.TakeErr(err);
            if (status == Native.ErrPipelineStopped) throw new PipelineStoppedException();
            if (status == Native.ErrNativePanic) throw new PwrsException("native module panicked: " + (error as string ?? "no message"));
            throw new WrappedHookFailed();
        }
    }

    /// <summary>
    /// The last stage of a wrapped command's steppable pipeline: passes each
    /// record the command writes to the wrapping cmdlet's hook.
    /// </summary>
    [Cmdlet("Out", "PwrsWrapped")]
    public sealed class WrappedOutputCommand : PSCmdlet
    {
        [Parameter(Mandatory = true)]
        public WrappedCmdlet? Owner { get; set; }

        [Parameter(ValueFromPipeline = true)]
        public object? InputObject { get; set; }

        protected override void ProcessRecord()
        {
            if (Owner == null) throw new InvalidOperationException("pwrs: the wrapped command's last stage has no wrapping cmdlet");
            Owner.Deliver(InputObject);
        }
    }

    /// <summary>
    /// Ends a wrapped command whose output hook failed, the native side
    /// having stored the error it reports.
    /// </summary>
    internal sealed class WrappedHookFailed : Exception
    {
        internal WrappedHookFailed() : base("pwrs: the wrapping cmdlet's output hook failed") { }
    }

    /// <summary>A type a wrapped command's attribute names that the running engine lacks.</summary>
    internal sealed class MissingTypeException : Exception
    {
    }
}
