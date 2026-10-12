using System;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// A gate one thread holds at a time, taken and opened by
    /// compare-exchange on the holder's managed thread id. An entry is
    /// shared or exclusive. The holder's shared entries nest; an
    /// exclusive entry is refused while any entry is inside, and every
    /// entry is refused while an exclusive one is. Another thread waits
    /// for the holder's outermost entry to leave: it spins briefly, then
    /// parks on an event the gate makes on its first contention, which a
    /// leaving holder sets while any thread is parked or about to park.
    /// </summary>
    internal sealed class Gate
    {
        /// <summary>The holder's managed thread id, or 0 while the gate is open.</summary>
        private int _owner;
        /// <summary>Entries the holder is inside; read and written by the holder alone.</summary>
        private int _depth;
        /// <summary>The holder's entry is exclusive; read and written by the holder alone.</summary>
        private bool _exclusive;
        /// <summary>Threads parked on <see cref="_parker"/> or about to park.</summary>
        private int _waiters;
        private AutoResetEvent? _parker;

        /// <summary>Whether the calling thread holds the gate.</summary>
        internal bool HeldByCurrentThread => Volatile.Read(ref _owner) == Environment.CurrentManagedThreadId;

        /// <summary>The entries the holder is inside, read by the holder.</summary>
        internal int Depth => _depth;

        /// <summary>
        /// Enters for the calling thread, waiting while another thread
        /// holds the gate. Answers false, entering nothing, when the
        /// calling thread holds the gate and the entry cannot nest: an
        /// exclusive entry, or any entry while an exclusive one is inside.
        /// Every true answer is paired with <see cref="Leave"/> on the
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
        /// thread waits. Called by the holder. Throws nothing.
        /// </summary>
        internal void Leave()
        {
            if (--_depth > 0) return;
            _exclusive = false;
            Interlocked.Exchange(ref _owner, 0);
            if (Volatile.Read(ref _waiters) > 0) Parker().Set();
        }

        /// <summary>
        /// Returns once <paramref name="me"/> holds the gate. A waiter
        /// counts itself before its last attempt, and a holder opens the
        /// gate before it reads the count, so either the attempt finds the
        /// gate open or the holder sets the parker; the parker stays set
        /// for a waiter that has not reached it yet.
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
