#if NET
using System;
using System.Buffers;
using System.Threading;

namespace Pwrs
{
    /// <summary>
    /// Exposes native memory as Memory&lt;T&gt; without copying. The
    /// release callback runs exactly once, with the context it was
    /// given, when the manager is disposed or finalized, so the memory
    /// outlives every view taken before then. GetSpan and Pin throw
    /// ObjectDisposedException once it has run, and while the refusal
    /// byte the module controls is nonzero.
    /// </summary>
    internal sealed unsafe class RustMemoryManager<T> : MemoryManager<T> where T : unmanaged
    {
        private readonly void* _ptr;
        private readonly int _length;
        private readonly byte* _refused;
        private readonly void* _context;
        private readonly delegate* unmanaged[Cdecl]<void*, void> _release;
        private int _released;

        public RustMemoryManager(void* ptr, int length, byte* refused, void* context, delegate* unmanaged[Cdecl]<void*, void> release)
        {
            _ptr = ptr;
            _length = length;
            _refused = refused;
            _context = context;
            _release = release;
        }

        public override Span<T> GetSpan()
        {
            ThrowIfUnusable();
            return new Span<T>(_ptr, _length);
        }

        public override MemoryHandle Pin(int elementIndex = 0)
        {
            ThrowIfUnusable();
            if ((uint)elementIndex > (uint)_length) throw new ArgumentOutOfRangeException(nameof(elementIndex));
            return new MemoryHandle((T*)_ptr + elementIndex);
        }

        public override void Unpin() { }

        /// <summary>
        /// The refusal byte is read only while the manager is unreleased:
        /// release frees what holds it.
        /// </summary>
        private void ThrowIfUnusable()
        {
            if (Volatile.Read(ref _released) != 0) throw HostVTable.MemoryReleased();
            if (_refused != null && Volatile.Read(ref *_refused) != 0) throw HostVTable.MemoryRevoked();
        }

        protected override void Dispose(bool disposing)
        {
            if (Interlocked.Exchange(ref _released, 1) != 0) return;
            if (_release != null) _release(_context);
        }

        ~RustMemoryManager() => Dispose(false);
    }
}
#endif
