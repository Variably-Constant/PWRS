using System;
using System.Management.Automation;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

namespace Pwrs
{
    /// <summary>
    /// The managed half of a task a #[psmethods] method is handed: the
    /// TaskCompletionSource whose Task the generated method returns, and a
    /// native byte set once the caller's CancellationToken asks for
    /// cancellation, which the Rust half reads without a crossing. The
    /// first settle wins: it completes the task, stops listening to the
    /// token, and frees the byte. Continuations run asynchronously, so none
    /// runs on the Rust thread that settles the task.
    /// </summary>
    public abstract unsafe class TaskSource
    {
        private readonly CancellationToken _token;
        private readonly CancellationTokenRegistration _registration;
        private byte* _cancelled;
        private int _settled;

        protected TaskSource(CancellationToken token)
        {
            _token = token;
            _cancelled = (byte*)Marshal.AllocHGlobal(1);
            *_cancelled = 0;
            // Runs at once when the token is already canceled.
            _registration = token.Register(MarkCancelled);
        }

        private void MarkCancelled() => Volatile.Write(ref *_cancelled, (byte)1);

        /// <summary>The byte the Rust half reads; valid until the task is settled.</summary>
        public byte* CancelFlag => _cancelled;

        /// <summary>Whether this call is the one that settles the task.</summary>
        private bool Begin() => Interlocked.Exchange(ref _settled, 1) == 0;

        /// <summary>
        /// Stops listening to the token, waiting for a callback another
        /// thread is running, and only then frees the byte it writes.
        /// </summary>
        private void End()
        {
            _registration.Dispose();
            Marshal.FreeHGlobal((IntPtr)_cancelled);
            _cancelled = null;
        }

        internal void SetValue(object? value)
        {
            if (!Begin()) return;
            try
            {
                Complete(value);
            }
            catch (Exception e)
            {
                Fault(e);
            }
            finally
            {
                End();
            }
        }

        internal void SetFault(Exception e)
        {
            if (!Begin()) return;
            try
            {
                Fault(e);
            }
            finally
            {
                End();
            }
        }

        internal void SetCancel()
        {
            if (!Begin()) return;
            try
            {
                Cancel(_token);
            }
            finally
            {
                End();
            }
        }

        /// <summary>
        /// Faults the task with the exception the generated method is
        /// about to throw, when the call failed before or without handing
        /// it to Rust. A task Rust has settled already is left as it is.
        /// </summary>
        public void Discard(Exception e) => SetFault(e);

        protected abstract void Complete(object? value);
        protected abstract void Fault(Exception e);
        protected abstract void Cancel(CancellationToken token);
    }

    /// <summary>A task of <typeparamref name="T"/>; a task of no value is a TaskSource&lt;object&gt; completed with null.</summary>
    public sealed class TaskSource<T> : TaskSource
    {
        private readonly TaskCompletionSource<T> _source = new TaskCompletionSource<T>(TaskCreationOptions.RunContinuationsAsynchronously);

        public TaskSource(CancellationToken token) : base(token) { }

        public Task<T> Task => _source.Task;

        protected override void Complete(object? value) => _source.TrySetResult(value is T t ? t : (T)LanguagePrimitives.ConvertTo(value, typeof(T))!);

        protected override void Fault(Exception e) => _source.TrySetException(e);

        protected override void Cancel(CancellationToken token) => _source.TrySetCanceled(token);
    }
}
