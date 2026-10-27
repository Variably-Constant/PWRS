using System;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// A gate one thread at a time can be in, entered and opened by
    /// compare-exchange on that thread's managed thread id. An entry is
    /// shared or exclusive. The thread in the gate can nest shared
    /// entries; an exclusive entry is refused while any entry is inside,
    /// and every entry is refused while an exclusive one is. Another
    /// thread waits for the outermost entry to leave: it spins briefly,
    /// then parks on an event the gate makes on its first contention,
    /// which the leaving thread sets while any thread is parked or about
    /// to park.
    /// </summary>
    internal sealed class Gate
    {
        /// <summary>The managed thread id of the thread in the gate, or 0 while the gate is open.</summary>
        private int _owner;
        /// <summary>Entries the thread in the gate is inside; read and written by that thread alone.</summary>
        private int _depth;
        /// <summary>The entry of the thread in the gate is exclusive; read and written by that thread alone.</summary>
        private bool _exclusive;
        /// <summary>Threads parked on <see cref="_parker"/> or about to park.</summary>
        private int _waiters;
        private AutoResetEvent? _parker;

        /// <summary>Whether the calling thread is in the gate.</summary>
        internal bool HeldByCurrentThread => Volatile.Read(ref _owner) == Environment.CurrentManagedThreadId;

        /// <summary>The entries the thread in the gate is inside, read by that thread.</summary>
        internal int Depth => _depth;

        /// <summary>
        /// Enters for the calling thread, waiting while another thread
        /// is in the gate. Returns false, entering nothing, when the
        /// calling thread is in the gate and the entry cannot nest: an
        /// exclusive entry, or any entry while an exclusive one is inside.
        /// Every true return is paired with <see cref="Leave"/> on the
        /// same thread.
        /// </summary>
        internal bool Enter(bool exclusive)
        {
            int me = Environment.CurrentManagedThreadId;
            if (Volatile.Read(ref _owner) == me)
            {
                if (_exclusive || exclusive) return false;
                _depth++;
                return true;
            }
            if (Interlocked.CompareExchange(ref _owner, me, 0) != 0) Wait(me);
            _depth = 1;
            _exclusive = exclusive;
            return true;
        }

        /// <summary>
        /// Leaves the calling thread's innermost entry, and opens the gate
        /// when that entry was the outermost, setting the parker when a
        /// thread waits. Called by the thread in the gate. Throws nothing.
        /// </summary>
        internal void Leave()
        {
            if (--_depth > 0) return;
            _exclusive = false;
            Interlocked.Exchange(ref _owner, 0);
            if (Volatile.Read(ref _waiters) > 0) Parker().Set();
        }

        /// <summary>
        /// Returns once <paramref name="me"/> is in the gate. A waiter
        /// counts itself before its last attempt, and the thread leaving
        /// the gate opens it before it reads the count, so either the
        /// attempt finds the gate open or the leaving thread sets the
        /// parker; the parker stays set for a waiter that has not reached
        /// it yet.
        /// </summary>
        private void Wait(int me)
        {
            var spin = new SpinWait();
            while (!spin.NextSpinWillYield)
            {
                spin.SpinOnce();
                if (Volatile.Read(ref _owner) == 0 && Interlocked.CompareExchange(ref _owner, me, 0) == 0) return;
            }
            AutoResetEvent parker = Parker();
            Interlocked.Increment(ref _waiters);
            try
            {
                while (Interlocked.CompareExchange(ref _owner, me, 0) != 0) parker.WaitOne();
            }
            finally
            {
                Interlocked.Decrement(ref _waiters);
            }
        }

        /// <summary>
        /// The event waiters park on, made by whichever thread first needs
        /// it and published by compare-exchange, so every thread parks on
        /// and sets the same one.
        /// </summary>
        private AutoResetEvent Parker()
        {
            AutoResetEvent? parker = Volatile.Read(ref _parker);
            if (parker != null) return parker;
            var made = new AutoResetEvent(false);
            AutoResetEvent? first = Interlocked.CompareExchange(ref _parker, made, null);
            if (first == null) return made;
            made.Dispose();
            return first;
        }
    }
}
