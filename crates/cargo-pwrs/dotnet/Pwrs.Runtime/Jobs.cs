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
    /// </summary>
    public sealed unsafe class RustJob : Job2
    {
        /// <summary>The PSJobTypeName of every PWRS job, and its source adapter's Name.</summary>
        public const string TypeName = "RustJob";

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
        internal void Finish(bool failed, string message, string details)
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
            if (stopped)
            {
                SetJobState(JobState.Stopped);
            }
            else if (failed)
            {
                Exception reason = details.Length == 0 ? new PwrsException(message) : new PwrsException(message, new PwrsException(details));
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
    /// </summary>
    public abstract class RustJobSourceAdapter : JobSourceAdapter
    {
        /// <summary>The JobInvocationInfo parameter with the job to store.</summary>
        internal const string NewJobKey = "NewJob";

        private readonly ConcurrentDictionary<Guid, Job2> _jobs = new ConcurrentDictionary<Guid, Job2>();

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

        public override IList<Job2> GetJobs() => _jobs.Values.ToList();

        public override IList<Job2> GetJobsByName(string name, bool recurse) => Where(j => Matches(name, j.Name));

        public override IList<Job2> GetJobsByCommand(string command, bool recurse) => Where(j => Matches(command, j.Command));

        public override Job2? GetJobByInstanceId(Guid instanceId, bool recurse) => _jobs.TryGetValue(instanceId, out Job2? job) ? job : null;

        public override Job2? GetJobBySessionId(int id, bool recurse) => _jobs.Values.FirstOrDefault(j => j.Id == id);

        public override IList<Job2> GetJobsByState(JobState state, bool recurse) => Where(j => j.JobStateInfo.State == state);

        public override IList<Job2> GetJobsByFilter(Dictionary<string, object> filter, bool recurse) => new List<Job2>();

        public override void RemoveJob(Job2 job) => _jobs.TryRemove(job.InstanceId, out _);

        private IList<Job2> Where(Func<Job2, bool> keep) => _jobs.Values.Where(keep).ToList();

        private static bool Matches(string pattern, string value) => new WildcardPattern(pattern, WildcardOptions.IgnoreCase).IsMatch(value);
    }
}
