using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.ComponentModel;
using System.Linq;
using System.Management.Automation;
using System.Management.Automation.Runspaces;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// A job whose work is Rust code on a thread the module started. The
    /// native side makes it Running and writes its streams from that
    /// thread through the host table; it ends through job_finish. Stop-Job
    /// calls the native stop callback to request a stop and waits for
    /// it to end, so the job is Stopped once Stop-Job returns.
    ///
    /// The native context is released once, when the job finishes, and the
    /// stop callback is never called after that: both happen under one lock.
    ///
    /// Receive-Job -Wait -AutoRemoveJob removes a job twice when the job
    /// ends after the cmdlet has started waiting on it and before it has
    /// written what the job already stored, and reports the second removal
    /// as an error when no adapter knows the job any more. That second
    /// removal runs inside the job's own change to its final state, so
    /// the job tells the adapter removing it whether that change is being
    /// announced, and the adapter answers for the job until it is over.
    /// </summary>
    public sealed unsafe class RustJob : Job2, IServiceProvider
    {
        /// <summary>The PSJobTypeName of every PWRS job, and its source adapter's Name.</summary>
        public const string TypeName = "RustJob";

        /// <summary>The bit of _phase set while the final state is being announced.</summary>
        private const int Ending = 1;

        /// <summary>The bit of _phase set once an adapter's RemoveJob has taken the job.</summary>
        private const int Removed = 2;

#if NET
        private readonly delegate* unmanaged[Cdecl]<IntPtr, void> _stop;
        private readonly delegate* unmanaged[Cdecl]<IntPtr, void> _release;
#else
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void ContextFn(IntPtr context);
        private readonly ContextFn _stop;
        private readonly ContextFn _release;
#endif
        private readonly IntPtr _context;
        private readonly object _gate = new object();
        private readonly ManualResetEventSlim _finished = new ManualResetEventSlim(false);
        private bool _released;
        private bool _stopAsked;
        private int _phase;
        private Action? _forget;

        /// <summary>
        /// What an adapter asks the job as it removes it: a function that
        /// takes how the adapter forgets the job, and answers whether the
        /// job's final state is still being announced. When it is, the
        /// adapter keeps the job, and the job calls what it was given once
        /// the announcement is over.
        ///
        /// The function is a delegate of framework types, since the adapter
        /// may belong to another load of this runtime, which shares no type
        /// of its own with this one. PowerShell 7 loads a runtime for each
        /// module and for each rebuild of one, and the job manager keeps the
        /// adapter it first registered under a name.
        /// </summary>
        object? IServiceProvider.GetService(Type serviceType) =>
            serviceType == typeof(Func<Action, bool>) ? new Func<Action, bool>(RemovedWhileEnding) : null;

        private bool RemovedWhileEnding(Action forget)
        {
            Volatile.Write(ref _forget, forget);
            return (Flag(Removed, true) & Ending) != 0;
        }

        /// <summary>Sets or clears a bit of _phase, and returns _phase as that leaves it.</summary>
        private int Flag(int bit, bool set)
        {
            while (true)
            {
                int seen = Volatile.Read(ref _phase);
                int next = set ? seen | bit : seen & ~bit;
                if (Interlocked.CompareExchange(ref _phase, next, seen) == seen) return next;
            }
        }

        internal RustJob(string command, string? name, IntPtr stop, IntPtr context, IntPtr release)
            : base(command, name)
        {
            PSJobTypeName = TypeName;
#if NET
            _stop = (delegate* unmanaged[Cdecl]<IntPtr, void>)stop;
            _release = (delegate* unmanaged[Cdecl]<IntPtr, void>)release;
#else
            _stop = Marshal.GetDelegateForFunctionPointer<ContextFn>(stop);
            _release = Marshal.GetDelegateForFunctionPointer<ContextFn>(release);
#endif
            _context = context;
        }

        public override string StatusMessage => string.Empty;

        public override bool HasMoreData =>
            Output.Count > 0 || Error.Count > 0 || Warning.Count > 0 || Verbose.Count > 0 || Debug.Count > 0 || Information.Count > 0 || Progress.Count > 0;

        public override string Location => "PWRS";

        /// <summary>Marks the job Running; its thread is started by the native side.</summary>
        internal void Begin() => SetJobState(JobState.Running);

        /// <summary>
        /// Ends the job and releases the native context: Stopped when a stop
        /// was requested, else Failed with the message, else Completed.
        /// </summary>
        internal void Finish(bool failed, string message, string details) =>
            Finish(!failed ? null : details.Length == 0 ? new PwrsException(message) : new PwrsException(message, new PwrsException(details)));

        /// <summary>
        /// Ends the job and releases the native context: Stopped when a stop
        /// was requested, else Failed with <paramref name="reason"/> when it
        /// is not null, else Completed.
        /// </summary>
        internal void Finish(Exception? reason)
        {
            bool stopped;
            lock (_gate)
            {
                stopped = _stopAsked;
                if (!_released)
                {
                    _released = true;
                    _release(_context);
                }
            }
            Flag(Ending, true);
            try
            {
                if (stopped)
                {
                    SetJobState(JobState.Stopped);
                }
                else if (reason != null)
                {
#if NET
                    SetJobState(JobState.Failed, reason);
#else
                    SetFailed(reason);
#endif
                }
                else
                {
                    SetJobState(JobState.Completed);
                }
            }
            finally
            {
                if ((Flag(Ending, false) & Removed) != 0) Volatile.Read(ref _forget)?.Invoke();
            }
            _finished.Set();
        }

#if !NET
        /// <summary>
        /// Job2's protected SetJobState(JobState, Exception), which Windows
        /// PowerShell's System.Management.Automation has and the reference
        /// assemblies this half compiles against do not.
        /// </summary>
        private static readonly MethodInfo? SetJobStateWithReason = typeof(Job2).GetMethod(
            "SetJobState",
            BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic,
            null,
            new[] { typeof(JobState), typeof(Exception) },
            null);

        /// <summary>
        /// Failed with the reason; where the overload taking one is missing,
        /// the reason is an error record in the job's error stream instead.
        /// </summary>
        private void SetFailed(Exception reason)
        {
            if (SetJobStateWithReason != null)
            {
                SetJobStateWithReason.Invoke(this, new object[] { JobState.Failed, reason });
                return;
            }
            Error.Add(new ErrorRecord(reason, "PwrsJobFailed", ErrorCategory.NotSpecified, null));
            SetJobState(JobState.Failed);
        }
#endif

        /// <summary>The job's work runs from the moment the native side makes it.</summary>
        public override void StartJob() { }

        public override void StartJobAsync() => OnStartJobCompleted(new AsyncCompletedEventArgs(null, false, null));

        public override void StopJob() => StopJob(false, null);

        /// <summary>Requests a stop, once, and waits until the work has ended.</summary>
        public override void StopJob(bool force, string? reason)
        {
            lock (_gate)
            {
                if (!_released && !_stopAsked)
                {
                    _stopAsked = true;
                    _stop(_context);
                }
            }
            _finished.Wait();
        }

        public override void StopJobAsync() => StopJobAsync(false, null);

        public override void StopJobAsync(bool force, string? reason)
        {
            ThreadPool.QueueUserWorkItem(_ =>
            {
                Exception? failure = null;
                try { StopJob(force, reason); }
                catch (Exception e) { failure = e; }
                OnStopJobCompleted(new AsyncCompletedEventArgs(failure, false, null));
            });
        }

        public override void SuspendJob() => throw NotSupported("suspended");
        public override void SuspendJobAsync() => throw NotSupported("suspended");
        public override void SuspendJob(bool force, string? reason) => throw NotSupported("suspended");
        public override void SuspendJobAsync(bool force, string? reason) => throw NotSupported("suspended");
        public override void ResumeJob() => throw NotSupported("resumed");
        public override void ResumeJobAsync() => throw NotSupported("resumed");
        public override void UnblockJob() => throw NotSupported("unblocked");
        public override void UnblockJobAsync() => throw NotSupported("unblocked");

        private static NotSupportedException NotSupported(string verb) => new NotSupportedException($"a {TypeName} cannot be {verb}");
    }

    /// <summary>
    /// The base of the job source adapter each module's shell declares,
    /// which Import-Module registers with the job manager by the subclass's
    /// own name, so every module's RustJobs are found by Get-Job,
    /// Receive-Job -AutoRemoveJob and Remove-Job, and two modules' adapters
    /// never share a name. A RustJob is made by the module running it; the
    /// adapter only stores it.
    ///
    /// A job removed while its final state is being announced stays stored
    /// until that is over, answering by instance id and in no listing, so
    /// the job manager finds it for each of the two removals Receive-Job
    /// -Wait -AutoRemoveJob can make. The adapter asks the job through its
    /// IServiceProvider, never by its type: the job manager keeps the first
    /// adapter registered under a name, which may be another load's.
    /// </summary>
    public abstract class RustJobSourceAdapter : JobSourceAdapter
    {
        /// <summary>The JobInvocationInfo parameter with the job to store.</summary>
        internal const string NewJobKey = "NewJob";

        private readonly ConcurrentDictionary<Guid, Job2> _jobs = new ConcurrentDictionary<Guid, Job2>();

        /// <summary>The jobs RemoveJob left stored while their final state was being announced.</summary>
        private readonly ConcurrentDictionary<Guid, bool> _leaving = new ConcurrentDictionary<Guid, bool>();

        protected RustJobSourceAdapter()
        {
            Name = RustJob.TypeName;
        }

        public override Job2 NewJob(JobInvocationInfo specification)
        {
            foreach (CommandParameterCollection set in specification.Parameters)
            {
                foreach (CommandParameter p in set)
                {
                    if (p.Name == NewJobKey && p.Value is Job2 job)
                    {
                        _jobs[job.InstanceId] = job;
                        return job;
                    }
                }
            }
            throw new PwrsException($"a {RustJob.TypeName} is made by the module that runs it, not from a job definition");
        }

        public override IList<Job2> GetJobs() => Listed().ToList();

        public override IList<Job2> GetJobsByName(string name, bool recurse) => Where(j => Matches(name, j.Name));

        public override IList<Job2> GetJobsByCommand(string command, bool recurse) => Where(j => Matches(command, j.Command));

        /// <summary>
        /// The stored job with the id, a removed one whose final state is
        /// still being announced included: the job manager removes a job
        /// only when this finds it.
        /// </summary>
        public override Job2? GetJobByInstanceId(Guid instanceId, bool recurse) => _jobs.TryGetValue(instanceId, out Job2? job) ? job : null;

        public override Job2? GetJobBySessionId(int id, bool recurse) => Listed().FirstOrDefault(j => j.Id == id);

        public override IList<Job2> GetJobsByState(JobState state, bool recurse) => Where(j => j.JobStateInfo.State == state);

        public override IList<Job2> GetJobsByFilter(Dictionary<string, object> filter, bool recurse) => new List<Job2>();

        public override void RemoveJob(Job2 job)
        {
            Guid id = job.InstanceId;
            if ((job as IServiceProvider)?.GetService(typeof(Func<Action, bool>)) is Func<Action, bool> removedWhileEnding)
            {
                _leaving[id] = true;
                if (removedWhileEnding(() => Forget(id))) return;
            }
            Forget(id);
        }

        /// <summary>Drops a job from the store.</summary>
        private void Forget(Guid id)
        {
            _jobs.TryRemove(id, out _);
            _leaving.TryRemove(id, out _);
        }

        /// <summary>The stored jobs no RemoveJob has taken.</summary>
        private IEnumerable<Job2> Listed() => _jobs.Values.Where(j => !_leaving.ContainsKey(j.InstanceId));

        private IList<Job2> Where(Func<Job2, bool> keep) => Listed().Where(keep).ToList();

        private static bool Matches(string pattern, string value) => new WildcardPattern(pattern, WildcardOptions.IgnoreCase).IsMatch(value);
    }
}
