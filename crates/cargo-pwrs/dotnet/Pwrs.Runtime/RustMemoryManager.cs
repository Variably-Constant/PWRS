#if NET
using System;
using System.Buffers;
using System.Runtime.InteropServices;

namespace Pwrs
{
    /// <summary>
    /// Exposes Rust-owned memory as Memory&lt;T&gt; without copying. The
    /// drop callback runs once, when the manager is disposed or
    /// finalized, so the Rust allocation outlives every managed view.
    /// </summary>
    internal sealed unsafe class RustMemoryManager<T> : MemoryManager<T> where T : unmanaged
    {
        private void* _ptr;
        private readonly int _length;
        private delegate* unmanaged[Cdecl]<void*, void> _drop;

        public RustMemoryManager(void* ptr, int length, delegate* unmanaged[Cdecl]<void*, void> drop)
        {
            _ptr = ptr;
            _length = length;
            _drop = drop;
        }

        public override Span<T> GetSpan() => new Span<T>(_ptr, _length);

        public override MemoryHandle Pin(int elementIndex = 0)
        {
            if ((uint)elementIndex > (uint)_length) throw new ArgumentOutOfRangeException(nameof(elementIndex));
            return new MemoryHandle((T*)_ptr + elementIndex);
        }

        public override void Unpin() { }

        protected override void Dispose(bool disposing)
        {
            void* p = _ptr;
            _ptr = null;
            if (p != null && _drop != null) _drop(p);
        }

        ~RustMemoryManager() => Dispose(false);
    }
}
#endif
